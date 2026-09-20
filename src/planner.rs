use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{Context, bail};
use chrono::Utc;
use uuid::Uuid;

use crate::model::*;

const MAX_TARGET: u32 = 500;

pub fn production_plan(
    game: &Game,
    document: &Document,
    inventory: &InventoryFile,
    request: &ProductionPlanRequest,
) -> anyhow::Result<PrintPlan> {
    if request.target_games == 0 || request.target_games > MAX_TARGET {
        bail!("target_games must be between 1 and {MAX_TARGET}");
    }
    let selected = selected_elements(document, &request.selected_element_ids)?;
    let use_stock = request.inventory_mode == InventoryMode::UseInventory;
    let mut requirements = Vec::new();
    let mut remaining_stock = HashMap::new();

    for element in &selected {
        let have = inventory.get(&game.id, &element.id);
        let consumed = if use_stock {
            have.min(request.target_games)
        } else {
            0
        };
        let to_produce = request.target_games - consumed;
        let sheets = div_ceil(to_produce, document.copies_per_page);
        requirements.push(requirement(
            document,
            element,
            have,
            request.target_games,
            consumed,
            sheets,
        ));
        remaining_stock.insert(element.id.as_str(), consumed);
    }

    let mut sets = Vec::new();
    let mut items = Vec::new();
    if document.copies_per_page == 1 {
        for number in 1..=request.target_games {
            let mut set_elements = Vec::new();
            for element in &selected {
                let stock = remaining_stock
                    .get_mut(element.id.as_str())
                    .expect("selected element");
                let source = if *stock > 0 {
                    *stock -= 1;
                    ElementSource::Inventory
                } else {
                    items.push(plan_item(document, element, Some(number), 1));
                    ElementSource::Print
                };
                set_elements.push(SetElement {
                    element_id: element.id.clone(),
                    element_name: element.name.clone(),
                    page: element.page,
                    source,
                });
            }
            sets.push(PlanSet {
                number,
                elements: set_elements,
            });
        }
    } else {
        for (element, req) in selected.iter().zip(&requirements) {
            if req.sheets > 0 {
                items.push(plan_item(document, element, None, req.sheets));
            }
        }
    }

    Ok(with_print_settings(
        base_plan(
            game,
            document,
            PlanKind::Production,
            Some(request.target_games),
            request.inventory_mode,
            inventory.revision,
            PlanContents {
                requirements,
                sets,
                items,
            },
        ),
        request.group_by_profile,
        request.profile_override.clone(),
        request.profile_replacements.clone(),
    ))
}

pub fn manual_plan(
    game: &Game,
    document: &Document,
    inventory: &InventoryFile,
    request: &ManualPlanRequest,
) -> anyhow::Result<PrintPlan> {
    if request.selections.is_empty() {
        bail!("select at least one element");
    }
    let by_id: HashMap<_, _> = document
        .elements
        .iter()
        .map(|e| (e.id.as_str(), e))
        .collect();
    let mut seen = HashSet::new();
    let mut requirements = Vec::new();
    let mut items = Vec::new();
    for selection in &request.selections {
        if selection.quantity == 0 || selection.quantity > MAX_TARGET {
            bail!("quantity must be between 1 and {MAX_TARGET}");
        }
        if !seen.insert(selection.element_id.as_str()) {
            bail!("duplicate element '{}'", selection.element_id);
        }
        let element = by_id
            .get(selection.element_id.as_str())
            .with_context(|| format!("unknown element '{}'", selection.element_id))?;
        let sheets = div_ceil(selection.quantity, document.copies_per_page);
        requirements.push(requirement(
            document,
            element,
            inventory.get(&game.id, &element.id),
            selection.quantity,
            0,
            sheets,
        ));
        items.push(plan_item(document, element, None, sheets));
    }
    Ok(with_print_settings(
        base_plan(
            game,
            document,
            PlanKind::Manual,
            None,
            InventoryMode::IgnoreInventory,
            inventory.revision,
            PlanContents {
                requirements,
                sets: vec![],
                items,
            },
        ),
        request.group_by_profile,
        request.profile_override.clone(),
        request.profile_replacements.clone(),
    ))
}

fn with_print_settings(
    mut plan: PrintPlan,
    group: bool,
    profile: Option<String>,
    replacements: BTreeMap<String, String>,
) -> PrintPlan {
    plan.group_by_profile = group;
    plan.profile_override = profile.clone();
    let replace = |original: &str| {
        profile
            .as_ref()
            .or_else(|| replacements.get(original))
            .cloned()
            .unwrap_or_else(|| original.to_owned())
    };
    for item in &mut plan.items {
        item.profile = replace(&item.profile);
    }
    for requirement in &mut plan.requirements {
        requirement.profile = replace(&requirement.profile);
    }
    plan.profile_replacements = replacements;
    plan
}

fn selected_elements<'a>(
    document: &'a Document,
    ids: &[String],
) -> anyhow::Result<Vec<&'a Element>> {
    if ids.is_empty() {
        bail!("select at least one element");
    }
    let requested: HashSet<_> = ids.iter().map(String::as_str).collect();
    if requested.len() != ids.len() {
        bail!("selected elements contain duplicates");
    }
    let selected: Vec<_> = document
        .elements
        .iter()
        .filter(|e| requested.contains(e.id.as_str()))
        .collect();
    if selected.len() != requested.len() {
        bail!("one or more selected elements do not belong to this document");
    }
    Ok(selected)
}

fn div_ceil(value: u32, divisor: u32) -> u32 {
    if value == 0 {
        0
    } else {
        1 + (value - 1) / divisor
    }
}
fn profile(document: &Document, element: &Element) -> String {
    element
        .profile
        .clone()
        .unwrap_or_else(|| document.default_profile.clone())
}
fn plan_item(document: &Document, element: &Element, set: Option<u32>, quantity: u32) -> PlanItem {
    PlanItem {
        set,
        element_id: element.id.clone(),
        element_name: element.name.clone(),
        page: element.page,
        profile: profile(document, element),
        quantity,
    }
}
fn requirement(
    document: &Document,
    element: &Element,
    have: u32,
    need: u32,
    consume: u32,
    sheets: u32,
) -> ElementRequirement {
    let to_produce = need - consume;
    let produced = sheets * document.copies_per_page;
    ElementRequirement {
        element_id: element.id.clone(),
        element_name: element.name.clone(),
        episode: element.episode.clone(),
        page: element.page,
        profile: profile(document, element),
        have,
        need,
        consume_from_stock: consume,
        to_produce,
        sheets,
        produced,
        surplus: produced.saturating_sub(to_produce),
    }
}
fn base_plan(
    game: &Game,
    document: &Document,
    kind: PlanKind,
    target_games: Option<u32>,
    inventory_mode: InventoryMode,
    inventory_revision: u64,
    contents: PlanContents,
) -> PrintPlan {
    PrintPlan {
        id: Uuid::new_v4(),
        kind,
        created_at: Utc::now(),
        game_id: game.id.clone(),
        game_name: game.name.clone(),
        document_id: document.id.clone(),
        document_name: document.name.clone(),
        target_games,
        inventory_mode,
        inventory_revision,
        copies_per_page: document.copies_per_page,
        requirements: contents.requirements,
        sets: contents.sets,
        items: contents.items,
        group_by_profile: false,
        profile_override: None,
        profile_replacements: BTreeMap::new(),
    }
}

struct PlanContents {
    requirements: Vec<ElementRequirement>,
    sets: Vec<PlanSet>,
    items: Vec<PlanItem>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn fixture(yield_per_page: u32) -> (Game, Document) {
        let doc = Document {
            id: "white".into(),
            name: "White".into(),
            file: "x.pdf".into(),
            default_profile: "normal".into(),
            copies_per_page: yield_per_page,
            elements: vec![
                Element {
                    id: "one".into(),
                    name: "One".into(),
                    page: 1,
                    episode: "ep1".into(),
                    profile: None,
                },
                Element {
                    id: "two".into(),
                    name: "Two".into(),
                    page: 2,
                    episode: "ep1".into(),
                    profile: Some("high".into()),
                },
            ],
        };
        let game = Game {
            id: "game".into(),
            name: "Game".into(),
            episodes: vec![],
            documents: vec![doc.clone()],
        };
        (game, doc)
    }
    #[test]
    fn stock_omits_earliest_set_without_destroying_order() {
        let (game, doc) = fixture(1);
        let mut inv = InventoryFile::default();
        inv.counts
            .insert("game".into(), BTreeMap::from([("two".into(), 1)]));
        let req = ProductionPlanRequest {
            game_id: "game".into(),
            document_id: "white".into(),
            target_games: 3,
            selected_element_ids: vec!["one".into(), "two".into()],
            inventory_mode: InventoryMode::UseInventory,
            group_by_profile: false,
            profile_override: None,
            profile_replacements: BTreeMap::new(),
        };
        let plan = production_plan(&game, &doc, &inv, &req).unwrap();
        assert_eq!(
            plan.items
                .iter()
                .map(|i| (i.set, i.page))
                .collect::<Vec<_>>(),
            vec![
                (Some(1), 1),
                (Some(2), 1),
                (Some(2), 2),
                (Some(3), 1),
                (Some(3), 2)
            ]
        );
        assert_eq!(plan.sets[0].elements[1].source, ElementSource::Inventory);
    }
    #[test]
    fn nup_rounds_to_whole_sheets_and_reports_surplus() {
        let (game, doc) = fixture(8);
        let req = ProductionPlanRequest {
            game_id: "game".into(),
            document_id: "white".into(),
            target_games: 10,
            selected_element_ids: vec!["one".into()],
            inventory_mode: InventoryMode::IgnoreInventory,
            group_by_profile: false,
            profile_override: None,
            profile_replacements: BTreeMap::new(),
        };
        let plan = production_plan(&game, &doc, &InventoryFile::default(), &req).unwrap();
        assert_eq!(
            (plan.requirements[0].sheets, plan.requirements[0].surplus),
            (2, 6)
        );
        assert_eq!(plan.items[0].quantity, 2);
    }

    #[test]
    fn ignore_inventory_prints_every_selected_element() {
        let (game, doc) = fixture(1);
        let mut inventory = InventoryFile::default();
        inventory.counts.insert(
            "game".into(),
            BTreeMap::from([("one".into(), 50), ("two".into(), 50)]),
        );
        let request = ProductionPlanRequest {
            game_id: "game".into(),
            document_id: "white".into(),
            target_games: 2,
            selected_element_ids: vec!["one".into(), "two".into()],
            inventory_mode: InventoryMode::IgnoreInventory,
            group_by_profile: false,
            profile_override: None,
            profile_replacements: BTreeMap::new(),
        };
        let plan = production_plan(&game, &doc, &inventory, &request).unwrap();
        assert_eq!(plan.items.len(), 4);
        assert!(plan.requirements.iter().all(|r| r.consume_from_stock == 0));
    }
}
