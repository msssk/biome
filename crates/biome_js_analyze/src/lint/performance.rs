//! Generated file, do not edit by hand, see `xtask/codegen`

use biome_analyze::declare_lint_group;

pub mod no_accumulating_spread;
pub mod no_delete;

declare_lint_group! {
    pub Performance {
        name : "performance" ,
        rules : [
            self :: no_accumulating_spread :: NoAccumulatingSpread ,
            self :: no_delete :: NoDelete ,
        ]
     }
}
