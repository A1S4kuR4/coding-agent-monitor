mod loader;
mod parser;
mod paths;
mod proto;
mod report;

use ccusage_adapter_common::filter_loaded_entries_by_date;
use ccusage_core::*;
use ccusage_cli::AgentCommandArgs;

pub use loader::load_entries;
pub use report::summarize_entries;
pub(crate) use report::report_from_rows;

pub fn run(args: AgentCommandArgs) -> Result<()> {
    let shared = args.shared;
    let pricing = PricingMap::load_with_overrides(
        shared.offline,
        log_level() != Some(0),
        shared.pricing_overrides.iter(),
    );
    let mut entries = load_entries(&shared, &pricing)?;
    filter_loaded_entries_by_date(&mut entries, &shared);
    let mut rows = summarize_entries(&entries, args.kind)?;
    sort_summaries(&mut rows, &shared.order, summary_period);
    if wants_json(&shared) {
        return print_json_or_jq(
            report_from_rows(&rows, args.kind),
            shared.jq.as_deref(),
            shared.no_cost,
        );
    }
    print_usage_table(
        "Antigravity Token Usage Report",
        first_column(args.kind),
        &rows,
        &shared,
        false,
        None,
    )
}
