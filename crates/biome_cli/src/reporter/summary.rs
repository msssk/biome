use crate::reporter::terminal::ConsoleTraversalSummary;
use crate::{DiagnosticsPayload, Execution, Reporter, ReporterVisitor, TraversalSummary};
use biome_console::fmt::{Display, Formatter};
use biome_console::{markup, Console, ConsoleExt};
use biome_diagnostics::Resource;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::io;

pub(crate) struct SummaryReporter {
    pub(crate) summary: TraversalSummary,
    pub(crate) diagnostics_payload: DiagnosticsPayload,
    pub(crate) execution: Execution,
}

impl Reporter for SummaryReporter {
    fn write(self, visitor: &mut dyn ReporterVisitor) -> io::Result<()> {
        visitor.report_diagnostics(&self.execution, self.diagnostics_payload)?;
        visitor.report_summary(&self.execution, self.summary)?;
        Ok(())
    }
}

pub(crate) struct SummaryReporterVisitor<'a>(pub(crate) &'a mut dyn Console);

impl<'a> ReporterVisitor for SummaryReporterVisitor<'a> {
    fn report_summary(
        &mut self,
        execution: &Execution,
        summary: TraversalSummary,
    ) -> io::Result<()> {
        if execution.is_check() && summary.suggested_fixes_skipped > 0 {
            self.0.log(markup! {
                <Warn>"Skipped "{summary.suggested_fixes_skipped}" suggested fixes.\n"</Warn>
                <Info>"If you wish to apply the suggested (unsafe) fixes, use the command "<Emphasis>"biome check --apply-unsafe\n"</Emphasis></Info>
            })
        }

        if !execution.is_ci() && summary.diagnostics_not_printed > 0 {
            self.0.log(markup! {
                <Warn>"The number of diagnostics exceeds the number allowed by Biome.\n"</Warn>
                <Info>"Diagnostics not shown: "</Info><Emphasis>{summary.diagnostics_not_printed}</Emphasis><Info>"."</Info>
            })
        }

        self.0.log(markup! {
            {ConsoleTraversalSummary(execution.traversal_mode(), &summary)}
        });

        Ok(())
    }

    fn report_diagnostics(
        &mut self,
        execution: &Execution,
        diagnostics_payload: DiagnosticsPayload,
    ) -> io::Result<()> {
        let mut files_to_diagnostics = FileToDiagnostics::default();

        for diagnostic in &diagnostics_payload.diagnostics {
            let location = diagnostic.location().resource.and_then(|r| match r {
                Resource::File(p) => Some(p),
                _ => None,
            });
            let Some(location) = location else {
                continue;
            };
            files_to_diagnostics.track_file(location);

            let category = diagnostic.category();
            if diagnostic.severity() >= diagnostics_payload.diagnostic_level {
                if diagnostic.tags().is_verbose() {
                    if diagnostics_payload.verbose {
                        if execution.is_check() || execution.is_lint() {
                            if let Some(category) = category {
                                if category.name().starts_with("lint/") {
                                    files_to_diagnostics.insert_lint(location, category.name());
                                }
                            }
                        }
                    } else {
                        continue;
                    }
                }

                if execution.is_check() || execution.is_lint() || execution.is_ci() {
                    if let Some(category) = category {
                        if category.name().starts_with("lint/") {
                            files_to_diagnostics.insert_lint(location, category.name());
                        }
                    }
                }

                if execution.is_check() || execution.is_format() || execution.is_ci() {
                    if let Some(category) = category {
                        if category.name().starts_with("format") {
                            files_to_diagnostics.insert_format(location);
                        }
                    }
                }
            }
        }

        self.0.log(markup! {{files_to_diagnostics}});
        // self.0.log(markup! {{formats_by_resource}});
        // self.0.log(markup! {{lints_by_category}});

        Ok(())
    }
}

#[derive(Debug, Default)]
struct FileToDiagnostics(BTreeMap<String, SummaryDiagnostics>);

impl FileToDiagnostics {
    fn track_file(&mut self, file_name: &str) {
        if !self.0.contains_key(file_name) {
            let file_name = file_name.into();
            self.0.insert(file_name, SummaryDiagnostics::default());
        }
    }

    fn get_summary(&mut self, file_name: &str) -> &mut SummaryDiagnostics {
        self.0.get_mut(file_name).expect("The file to be tracked")
    }

    fn insert_lint(&mut self, location: &str, rule_name: impl Into<RuleName>) {
        let summary = self.get_summary(location);
        let rule_name = rule_name.into();
        if let Some(value) = summary.lints.0.get_mut(&rule_name) {
            *value += 1;
        } else {
            summary.lints.0.insert(rule_name, 1);
        }
    }

    fn insert_format(&mut self, location: &str) {
        let summary = self.get_summary(location);
        summary.formats += 1;
    }
}

impl Display for FileToDiagnostics {
    fn fmt(&self, fmt: &mut Formatter) -> io::Result<()> {
        fmt.write_markup(markup! {
            <Info>"Summarised report of diagnostics by file."</Info>
        })?;
        fmt.write_str("\n\n")?;
        for (file_name, summary) in &self.0 {
            fmt.write_markup(markup! {
                "▶ "<Emphasis>{file_name}</Emphasis>"\n"
            })?;
            fmt.write_markup(markup! {
                {summary}
            })?;
            fmt.write_str("\n")?;
        }

        Ok(())
    }
}

#[derive(Debug, Default)]
struct SummaryDiagnostics {
    lints: LintsByCategory,
    formats: usize,
}

impl SummaryDiagnostics {}

impl Display for SummaryDiagnostics {
    fn fmt(&self, fmt: &mut Formatter) -> io::Result<()> {
        if self.formats > 0 {
            fmt.write_markup(markup! {
                {TAB}<Info>"The file isn't formatted."</Info>"\n\n"
            })?;
        }
        fmt.write_markup(markup! {
            {self.lints}
        })
    }
}

#[derive(Debug, Default)]
struct LintsByCategory(BTreeMap<RuleName, usize>);

impl Display for LintsByCategory {
    fn fmt(&self, fmt: &mut Formatter) -> io::Result<()> {
        let rule_name_str = "Rule Name";
        let diagnostics_str = "Diagnostics";
        let padding = 15usize;

        if !self.0.is_empty() {
            fmt.write_markup(markup!(
                {TAB}<Info>"Some lint rules were triggered"</Info>
            ))?;
            fmt.write_str("\n\n")?;
            let mut iter = self.0.iter().rev();
            // SAFETY: it isn't empty
            let (first_name, first_count) = iter.next().unwrap();
            let longest_rule_name = first_name.name_len();

            fmt.write_markup(markup!(
                {TAB}<Info><Underline>{rule_name_str}</Underline></Info>
            ))?;
            fmt.write_markup(markup! {{Padding(longest_rule_name + padding)}})?;
            fmt.write_markup(markup!(
                <Info><Dim>{diagnostics_str}</Dim></Info>
            ))?;
            fmt.write_str("\n")?;

            fmt.write_markup(markup! {
                {TAB}<Emphasis>{first_name}</Emphasis>{Padding(padding + rule_name_str.len())}{first_count}
            })?;

            fmt.write_str("\n")?;

            for (name, num) in iter {
                let current_name_len = name.name_len();
                let extra_padding = longest_rule_name.saturating_sub(current_name_len);
                fmt.write_markup(markup! {
                    {TAB}<Emphasis>{name}</Emphasis>
                })?;

                fmt.write_markup(markup! {
                    {Padding(extra_padding + padding + rule_name_str.len())}
                })?;

                fmt.write_markup(markup! {
                    {num}
                })?;
                fmt.write_str("\n")?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Default)]
struct RuleName(&'static str);

impl AsRef<str> for RuleName {
    fn as_ref(&self) -> &'static str {
        self.0
    }
}

impl RuleName {
    fn name_len(&self) -> usize {
        self.0.len()
    }
}

impl From<&'static str> for RuleName {
    fn from(value: &'static str) -> Self {
        Self(value)
    }
}

impl Eq for RuleName {}

impl PartialEq<Self> for RuleName {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl PartialOrd<Self> for RuleName {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.len().partial_cmp(&other.0.len())
    }
}

impl Ord for RuleName {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or_else(|| Ordering::Equal)
    }
}
impl Display for RuleName {
    fn fmt(&self, fmt: &mut Formatter) -> io::Result<()> {
        fmt.write_markup(markup!(
            <Emphasis>{self.0}</Emphasis>
        ))
    }
}

#[derive(Debug, Default)]
struct FormatsByFile(BTreeSet<String>);

impl Display for FormatsByFile {
    fn fmt(&self, fmt: &mut Formatter) -> io::Result<()> {
        if !self.0.is_empty() {
            fmt.write_markup(markup!(
                <Info>"Files that haven't been formatted yet"</Info>
            ))?;
            fmt.write_str("\n\n")?;

            for file_name in &self.0 {
                fmt.write_markup(markup! {
                    <Emphasis>{file_name}</Emphasis>
                })?;

                fmt.write_str("\n")?;
            }
        }
        Ok(())
    }
}

struct Padding(usize);
impl Display for Padding {
    fn fmt(&self, fmt: &mut Formatter) -> io::Result<()> {
        for _ in 0..self.0 {
            fmt.write_str(" ")?;
        }
        Ok(())
    }
}

const TAB: Tab = Tab(Padding(5));

struct Tab(Padding);

impl Display for Tab {
    fn fmt(&self, fmt: &mut Formatter) -> io::Result<()> {
        fmt.write_markup(markup! {{self.0}})
    }
}
