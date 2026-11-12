//! `balance_report` walks the tech tree and prints the raw materials each Era needs, so pacing
//! can be checked on paper without playing.
//!
//! For every tech node it recursively expands the node's item cost through the recipes that make
//! those items, down to raw ores and other un-craftable inputs. The output is grouped by Era and
//! printed as a sorted table, plus the total per raw material.
//!
//! Usage: `cargo run -p deepcore-server --bin balance_report -- [--content assets/data]`

use std::collections::HashMap;

use clap::Parser;
use deepcore_shared::content::{ContentId, ContentRegistry};
use deepcore_shared::crafting::TechTree;

/// Command line options.
#[derive(Debug, Parser)]
struct Args {
    /// Directory holding the RON content.
    #[arg(long, default_value = "assets/data")]
    content: String,
    /// How deep to expand recipes before giving up on a cycle.
    #[arg(long, default_value_t = 24)]
    depth: usize,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let content = ContentRegistry::load_from_dir(std::path::Path::new(&args.content))?;

    println!(
        "Deepcore balance report — {} items, {} recipes, {} machines, {} tech nodes, content hash {:016x}\n",
        content.items.len(),
        content.recipes.len(),
        content.machines.len(),
        content.tech.len(),
        content.content_hash
    );

    let mut by_era: HashMap<u8, HashMap<ContentId, u32>> = HashMap::new();
    let mut total: HashMap<ContentId, u32> = HashMap::new();

    for node in &content.tech {
        if node.cost.is_empty() {
            continue;
        }
        let mut raw = HashMap::new();
        for (item, count) in &node.cost {
            expand(&content, *item, *count, args.depth, &mut raw);
        }
        let era = by_era.entry(node.era).or_default();
        for (item, count) in &raw {
            *era.entry(*item).or_insert(0) += *count;
            *total.entry(*item).or_insert(0) += *count;
        }
    }

    for era in 0..=5u8 {
        let Some(materials) = by_era.get(&era) else {
            continue;
        };
        let nodes: Vec<&str> = content
            .tech
            .iter()
            .filter(|node| node.era == era)
            .map(|node| node.name.as_str())
            .collect();
        println!("== Era {era}: {} ==", nodes.join(", "));
        print_table(&content, materials);
        println!();
    }

    println!("== Totals across all Eras ==");
    print_table(&content, &total);

    // The nuclear chain, step by step, so the long tail is visible.
    println!("\n== Nuclear chain input cost per unit ==");
    for key in [
        "crushed_uranium_ore",
        "uranium_solution",
        "yellowcake",
        "uranium_hexafluoride",
        "enriched_canister",
        "fuel_rod",
        "compact_core_cell",
        "nuclear_drill",
    ] {
        let Some(item) = content.item_by_key(key) else {
            continue;
        };
        let mut raw = HashMap::new();
        expand(&content, item.id, 1, args.depth, &mut raw);
        let summary: Vec<String> = sorted(&content, &raw)
            .into_iter()
            .map(|(id, count)| format!("{} x{count}", name(&content, id)))
            .collect();
        println!("  {}: {}", item.name, summary.join(", "));
    }

    // Tech tree shape.
    println!("\n== Tech tree ==");
    let mut tech = TechTree::default();
    for node in &content.tech {
        let parents_done = node
            .parents
            .iter()
            .all(|parent| tech.is_researched(*parent));
        let mark = if node.parents.is_empty() || parents_done {
            " "
        } else {
            "!"
        };
        println!(
            "  {mark} era {} {:<24} parents: {}",
            node.era,
            node.name,
            if node.parents.is_empty() {
                "-".to_owned()
            } else {
                node.parents
                    .iter()
                    .filter_map(|parent| content.tech(*parent).map(|tech| tech.name.clone()))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
        tech.set_researched(vec![node.id]);
    }
    Ok(())
}

fn expand(
    content: &ContentRegistry,
    item: ContentId,
    count: u32,
    depth: usize,
    out: &mut HashMap<ContentId, u32>,
) {
    if depth == 0 {
        *out.entry(item).or_insert(0) += count;
        return;
    }
    // Prefer a recipe that makes this item; hand recipes first so raw ore is charged at the
    // cheapest realistic path.
    let Some(recipes) = content.recipes_by_output.get(&item) else {
        *out.entry(item).or_insert(0) += count;
        return;
    };
    let Some(recipe) = recipes
        .iter()
        .filter_map(|recipe| content.recipe(*recipe))
        .min_by(|a, b| a.time.total_cmp(&b.time))
    else {
        *out.entry(item).or_insert(0) += count;
        return;
    };
    let produced: u32 = recipe
        .outputs
        .iter()
        .filter(|(output, _)| *output == item)
        .map(|(_, amount)| *amount)
        .sum();
    if produced == 0 {
        *out.entry(item).or_insert(0) += count;
        return;
    }
    let batches = count.div_ceil(produced);
    for (input, amount) in &recipe.inputs {
        expand(content, *input, amount * batches, depth - 1, out);
    }
}

fn sorted(content: &ContentRegistry, map: &HashMap<ContentId, u32>) -> Vec<(ContentId, u32)> {
    let mut entries: Vec<(ContentId, u32)> = map.iter().map(|(id, count)| (*id, *count)).collect();
    entries.sort_by_key(|(id, _)| name(content, *id));
    entries
}

fn print_table(content: &ContentRegistry, map: &HashMap<ContentId, u32>) {
    for (id, count) in sorted(content, map) {
        println!("  {:<28} {:>8}", name(content, id), count);
    }
}

fn name(content: &ContentRegistry, id: ContentId) -> String {
    content
        .item(id)
        .map(|item| item.name.clone())
        .unwrap_or_else(|| format!("<{id}>"))
}
