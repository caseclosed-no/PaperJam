use std::{collections::BTreeMap, path::Path};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::{
    model::{Document, PrintPlan, PrinterConfig},
    store,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintJob {
    pub destination: String,
    pub server: Option<String>,
    pub file: String,
    pub title: String,
    pub page_start: u32,
    pub page_end: u32,
    /// Exact source pages (including repeats) for a grouped job. Empty for legacy jobs.
    #[serde(default)]
    pub source_pages: Vec<u32>,
    pub copies: u32,
    pub profile_id: String,
    pub options: BTreeMap<String, String>,
    pub set: Option<u32>,
    pub elements: Vec<String>,
}

impl PrintJob {
    pub fn page_range(&self) -> String {
        if !self.source_pages.is_empty() {
            return format!("1-{}", self.source_pages.len());
        }
        if self.page_start == self.page_end {
            self.page_start.to_string()
        } else {
            format!("{}-{}", self.page_start, self.page_end)
        }
    }
    pub fn lp_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(server) = &self.server {
            args.extend(["-h".into(), server.clone()]);
        }
        args.extend([
            "-d".into(),
            self.destination.clone(),
            "-t".into(),
            self.title.clone(),
            "-P".into(),
            self.page_range(),
        ]);
        if self.copies > 1 {
            args.extend(["-n".into(), self.copies.to_string()]);
        }
        for (key, value) in &self.options {
            args.extend(["-o".into(), format!("{key}={value}")]);
        }
        args.extend(["--".into(), self.file.clone()]);
        args
    }
}

pub fn compile_jobs(
    document: &Document,
    plan: &PrintPlan,
    printer: &PrinterConfig,
    pdf_root: &Path,
) -> anyhow::Result<Vec<PrintJob>> {
    let file = store::safe_pdf_path(pdf_root, &document.file)?
        .to_string_lossy()
        .into_owned();
    anyhow::ensure!(Path::new(&file).exists(), "PDF is unavailable: {file}");
    if let Some(id) = &plan.profile_override {
        anyhow::ensure!(
            printer.profiles.contains_key(id),
            "unknown profile '{}'",
            id
        );
    }
    for id in plan
        .profile_replacements
        .keys()
        .chain(plan.profile_replacements.values())
    {
        anyhow::ensure!(
            printer.profiles.contains_key(id),
            "unknown profile '{}'",
            id
        );
    }
    let mut jobs: Vec<PrintJob> = Vec::new();
    for item in &plan.items {
        let profile = printer
            .profiles
            .get(&item.profile)
            .with_context(|| format!("unknown profile '{}'", item.profile))?;
        let mut options = printer.default_options.clone();
        // An explicit portrait request disables CUPS' implicit auto-rotation.
        // Enable it explicitly so mixed-orientation pages fit the chosen sheet.
        // A configured false value still permits deliberately fixed orientation.
        options
            .entry("pdfAutorotate".into())
            .or_insert_with(|| "true".into());
        options.extend(profile.options.clone());
        let candidate = PrintJob {
            destination: printer.destination.clone(),
            server: printer.server.clone(),
            file: file.clone(),
            title: format!(
                "{} / {} / {} / page {}",
                plan.game_name,
                plan.document_name,
                item.set
                    .map(|n| format!("set {n}"))
                    .unwrap_or_else(|| "manual/sheets".into()),
                item.page
            ),
            page_start: item.page,
            page_end: item.page,
            source_pages: vec![],
            copies: item.quantity,
            profile_id: item.profile.clone(),
            options,
            set: item.set,
            elements: vec![item.element_name.clone()],
        };
        if plan.group_by_profile {
            let index = match jobs.iter().position(|job| job.profile_id == item.profile) {
                Some(index) => index,
                None => {
                    let mut grouped = candidate;
                    grouped.title = format!(
                        "{} / {} / {}",
                        plan.game_name, plan.document_name, profile.name
                    );
                    grouped.set = None;
                    grouped.copies = 1;
                    grouped.elements.clear();
                    jobs.push(grouped);
                    jobs.len() - 1
                }
            };
            let grouped = &mut jobs[index];
            grouped
                .source_pages
                .extend(std::iter::repeat_n(item.page, item.quantity as usize));
            grouped.page_start = grouped.page_start.min(item.page);
            grouped.page_end = grouped.page_end.max(item.page);
            if !grouped.elements.contains(&item.element_name) {
                grouped.elements.push(item.element_name.clone());
            }
            continue;
        }
        if document.copies_per_page == 1
            && let Some(previous) = jobs.last_mut()
            && can_merge(previous, &candidate)
        {
            previous.page_end = candidate.page_end;
            previous.elements.extend(candidate.elements);
            continue;
        }
        jobs.push(candidate);
    }
    Ok(jobs)
}

fn can_merge(a: &PrintJob, b: &PrintJob) -> bool {
    a.set == b.set
        && a.set.is_some()
        && a.file == b.file
        && a.profile_id == b.profile_id
        && a.copies == 1
        && b.copies == 1
        && b.page_start == a.page_end + 1
}

/// Build an exact page sequence without depending on CUPS page-range sorting or
/// uniform copy counts. Flatten inherited page properties before reparenting.
fn grouped_pdf(file: &Path, pages: &[u32]) -> anyhow::Result<lopdf::Document> {
    use lopdf::{Object, dictionary};
    let mut pdf = lopdf::Document::load(file).context("loading PDF for grouped printing")?;
    let source = pdf.get_pages();
    let root = pdf.new_object_id();
    let mut kids = Vec::with_capacity(pages.len());
    for number in pages {
        let id = source
            .get(number)
            .with_context(|| format!("PDF has no page {number}"))?;
        let mut page = pdf.get_dictionary(*id)?.clone();
        let mut parent = page.get(b"Parent").and_then(Object::as_reference).ok();
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = parent {
            anyhow::ensure!(seen.insert(id), "cyclic PDF page tree");
            let ancestor = pdf.get_dictionary(id)?;
            for key in [b"Resources".as_slice(), b"MediaBox", b"CropBox", b"Rotate"] {
                if !page.has(key)
                    && let Ok(value) = ancestor.get(key)
                {
                    page.set(key, value.clone());
                }
            }
            parent = ancestor.get(b"Parent").and_then(Object::as_reference).ok();
        }
        page.set("Parent", root);
        kids.push(Object::Reference(pdf.add_object(page)));
    }
    pdf.objects.insert(
        root,
        dictionary! {"Type" => "Pages", "Kids" => kids, "Count" => pages.len() as i64}.into(),
    );
    let mut catalog = pdf.catalog()?.clone();
    catalog.set("Pages", root);
    // Preserve color output intents and form appearance settings, but discard
    // navigation tied to the old page sequence.
    for key in [b"Outlines".as_slice(), b"PageLabels", b"OpenAction"] {
        catalog.remove(key);
    }
    let catalog = pdf.add_object(catalog);
    pdf.trailer.set("Root", catalog);
    Ok(pdf)
}

struct PreparedPdf(std::path::PathBuf);
impl Drop for PreparedPdf {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub async fn submit(job: &PrintJob, config: &PrinterConfig) -> anyhow::Result<Option<String>> {
    if config.backend == "dry-run" {
        return Ok(None);
    }
    let mut submission = job.clone();
    let _prepared = if job.source_pages.is_empty() {
        None
    } else {
        let file = job.file.clone();
        let pages = job.source_pages.clone();
        let prepared = tokio::task::spawn_blocking(move || -> anyhow::Result<PreparedPdf> {
            let path = std::env::temp_dir().join(format!("paperjam-{}.pdf", uuid::Uuid::new_v4()));
            let prepared = PreparedPdf(path);
            grouped_pdf(Path::new(&file), &pages)?.save(&prepared.0)?;
            Ok(prepared)
        })
        .await??;
        submission.file = prepared.0.to_string_lossy().into_owned();
        Some(prepared)
    };
    let output = Command::new("lp")
        .args(submission.lp_args())
        .output()
        .await
        .with_context(|| format!("starting lp for {}", job.title))?;
    if !output.status.success() {
        anyhow::bail!(
            "lp failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(Some(parse_request_id(&stdout, &job.destination)?))
}

#[derive(Debug, PartialEq, Eq)]
pub enum WaitOutcome {
    Completed,
    Retry,
}

pub async fn wait_until_complete(
    request_id: &str,
    config: &PrinterConfig,
    canceled: impl Fn() -> bool,
    recover: impl Fn() -> bool,
) -> anyhow::Result<WaitOutcome> {
    loop {
        if canceled() {
            cancel(request_id, config).await?;
            anyhow::bail!("canceled");
        }
        if recover() {
            // Never resubmit unless cancellation succeeded.
            cancel(request_id, config).await?;
            return Ok(WaitOutcome::Retry);
        }
        if config.backend == "dry-run" {
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            return Ok(WaitOutcome::Completed);
        }
        let mut command = Command::new("lpstat");
        if let Some(server) = &config.server {
            command.args(["-h", server]);
        }
        let output = command
            .args(["-W", "not-completed", "-o", &config.destination])
            .output()
            .await
            .context("checking CUPS job status")?;
        if !output.status.success() {
            anyhow::bail!(
                "lpstat failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        if !String::from_utf8_lossy(&output.stdout).contains(request_id) {
            return Ok(WaitOutcome::Completed);
        }
        tokio::time::sleep(std::time::Duration::from_millis(config.poll_interval_ms)).await;
    }
}

pub async fn cancel(request_id: &str, config: &PrinterConfig) -> anyhow::Result<()> {
    if config.backend == "dry-run" {
        return Ok(());
    }
    let mut command = Command::new("cancel");
    if let Some(server) = &config.server {
        command.args(["-h", server]);
    }
    let output = command
        .arg(request_id)
        .output()
        .await
        .context("canceling CUPS job")?;
    anyhow::ensure!(
        output.status.success(),
        "cancel failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

pub async fn status(config: &PrinterConfig) -> anyhow::Result<String> {
    if config.backend == "dry-run" {
        return Ok("Dry-run backend ready".into());
    }
    let mut command = Command::new("lpstat");
    if let Some(server) = &config.server {
        command.args(["-h", server]);
    }
    let output = command
        .args(["-p", &config.destination, "-l"])
        .output()
        .await
        .context("reading printer status")?;
    anyhow::ensure!(
        output.status.success(),
        "lpstat failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}

fn parse_request_id(stdout: &str, destination: &str) -> anyhow::Result<String> {
    for token in stdout.split_whitespace() {
        let token = token.trim_matches(|c: char| matches!(c, '(' | ')' | ','));
        if let Some((queue, n)) = token.rsplit_once('-')
            && queue.eq_ignore_ascii_case(destination)
            && n.parse::<u32>().is_ok()
        {
            return Ok(token.into());
        }
    }
    anyhow::bail!("no CUPS request ID found in {:?}", stdout.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn recovery_retries_and_cancel_takes_priority() {
        let f = fixture(1);
        assert_eq!(
            wait_until_complete("test-1", &f.printer, || false, || true)
                .await
                .unwrap(),
            WaitOutcome::Retry
        );
        assert!(
            wait_until_complete("test-1", &f.printer, || true, || panic!("cancel must win"))
                .await
                .is_err()
        );
        assert_eq!(
            wait_until_complete("test-1", &f.printer, || false, || false)
                .await
                .unwrap(),
            WaitOutcome::Completed
        );
    }

    #[test]
    fn parses_hyphenated_destination() {
        assert_eq!(
            parse_request_id("request id is MY-EPSON-42 (1 file(s))", "MY-EPSON").unwrap(),
            "MY-EPSON-42"
        );
    }

    struct Fixture {
        root: std::path::PathBuf,
        game: crate::model::Game,
        printer: PrinterConfig,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn fixture(yield_per_page: u32) -> Fixture {
        use lopdf::{Object, Stream, dictionary};
        let root =
            std::env::temp_dir().join(format!("paperjam-group-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut pdf = lopdf::Document::with_version("1.5");
        let pages = pdf.new_object_id();
        let resources = pdf.add_object(dictionary! {});
        let mut kids = Vec::new();
        for n in 1..=6 {
            let content = pdf.add_object(Stream::new(
                dictionary! {},
                format!("% page {n}\n").into_bytes(),
            ));
            let page =
                pdf.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages, "Contents"=>content});
            kids.push(Object::Reference(page));
        }
        pdf.objects.insert(pages, dictionary! {"Type"=>"Pages", "Kids"=>kids, "Count"=>6,
            "Resources"=>resources, "MediaBox"=>vec![0.into(),0.into(),595.into(),842.into()], "Rotate"=>90}.into());
        let catalog = pdf.add_object(
            dictionary! {"Type"=>"Catalog", "Pages"=>pages, "OutputIntents"=>Vec::<Object>::new()},
        );
        pdf.trailer.set("Root", catalog);
        pdf.save(root.join("source.pdf")).unwrap();
        let mut game: crate::model::Game = toml::from_str(
            r#"
            id = "test"
            name = "Test"
            [[episodes]]
            id = "main"
            name = "Main"
            [[documents]]
            id = "pages"
            name = "Pages"
            file = "source.pdf"
            default_profile = "standard"
            [[documents.elements]]
            id = "one"
            name = "One"
            page = 1
            episode = "main"
            [[documents.elements]]
            id = "three"
            name = "Three"
            page = 3
            episode = "main"
            profile = "high"
            [[documents.elements]]
            id = "six"
            name = "Six"
            page = 6
            episode = "main"
        "#,
        )
        .unwrap();
        game.documents[0].copies_per_page = yield_per_page;
        let printer = toml::from_str(
            r#"
            backend = "dry-run"
            destination = "test"
            [profiles.standard]
            name = "Standard"
            [profiles.standard.options]
            MediaType = "PLAIN_DRAFT"
            [profiles.high]
            name = "High"
            [profiles.high.options]
            MediaType = "PLAIN_HIGH"
        "#,
        )
        .unwrap();
        Fixture {
            root,
            game,
            printer,
        }
    }
    fn production_request() -> crate::model::ProductionPlanRequest {
        serde_json::from_value(serde_json::json!({
            "game_id":"test", "document_id":"pages", "target_games":3,
            "selected_element_ids":["one","three","six"],
            "inventory_mode":"use_inventory", "group_by_profile":true
        }))
        .unwrap()
    }
    #[test]
    fn groups_nonconsecutive_pages_with_stock_into_one_job_per_profile() {
        use crate::{model::InventoryFile, planner};
        let f = fixture(1);
        let doc = &f.game.documents[0];
        let mut inv = InventoryFile::default();
        inv.counts
            .insert("test".into(), BTreeMap::from([("one".into(), 1)]));
        let req = production_request();
        let plan = planner::production_plan(&f.game, doc, &inv, &req).unwrap();
        let jobs = compile_jobs(doc, &plan, &f.printer, &f.root).unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].profile_id, "high");
        assert_eq!(jobs[0].source_pages, vec![3, 3, 3]);
        assert_eq!(jobs[1].source_pages, vec![6, 1, 6, 1, 6]);
        assert_eq!(jobs[1].page_range(), "1-5");
        assert_eq!(jobs[1].copies, 1);
        assert!(jobs[1].lp_args().windows(2).any(|a| a == ["-P", "1-5"]));
        assert_eq!(plan.requirements[0].consume_from_stock, 1);
        let mut req = req;
        req.group_by_profile = false;
        let normal = planner::production_plan(&f.game, doc, &inv, &req).unwrap();
        let jobs = compile_jobs(doc, &normal, &f.printer, &f.root).unwrap();
        assert_eq!(jobs.len(), 8);
        assert!(jobs.iter().all(|j| j.source_pages.is_empty()));
    }
    #[test]
    fn three_sets_are_grouped_across_the_entire_run_and_autorotate_by_default() {
        use crate::{model::InventoryFile, planner};
        let mut f = fixture(1);
        let doc = &f.game.documents[0];
        let req = production_request();
        let plan = planner::production_plan(&f.game, doc, &InventoryFile::default(), &req).unwrap();
        let jobs = compile_jobs(doc, &plan, &f.printer, &f.root).unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].profile_id, "standard");
        assert_eq!(jobs[0].source_pages, vec![1, 6, 1, 6, 1, 6]);
        assert_eq!(jobs[1].profile_id, "high");
        assert_eq!(jobs[1].source_pages, vec![3, 3, 3]);
        assert!(
            jobs.iter()
                .all(|job| job.options["pdfAutorotate"] == "true")
        );
        // One copy of an assembled PDF already containing all three sets.
        assert!(jobs.iter().all(|job| job.copies == 1 && job.set.is_none()));
        let mut req = req;
        req.group_by_profile = false;
        let plan = planner::production_plan(&f.game, doc, &InventoryFile::default(), &req).unwrap();
        let jobs = compile_jobs(doc, &plan, &f.printer, &f.root).unwrap();
        assert!(
            jobs.iter()
                .all(|job| job.options["pdfAutorotate"] == "true")
        );
        f.printer
            .profiles
            .get_mut("high")
            .unwrap()
            .options
            .insert("pdfAutorotate".into(), "false".into());
        let jobs = compile_jobs(doc, &plan, &f.printer, &f.root).unwrap();
        assert!(
            jobs.iter()
                .filter(|job| job.profile_id == "high")
                .all(|job| job.options["pdfAutorotate"] == "false")
        );
    }

    #[test]
    fn override_applies_to_production_and_manual_without_changing_assignments() {
        use crate::{model::*, planner};
        let f = fixture(1);
        let doc = &f.game.documents[0];
        let mut req = production_request();
        req.profile_override = Some("high".into());
        let plan = planner::production_plan(&f.game, doc, &InventoryFile::default(), &req).unwrap();
        assert!(plan.requirements.iter().all(|r| r.profile == "high"));
        let jobs = compile_jobs(doc, &plan, &f.printer, &f.root).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].source_pages, vec![1, 3, 6, 1, 3, 6, 1, 3, 6]);
        assert_eq!(jobs[0].options["MediaType"], "PLAIN_HIGH");
        let req:ManualPlanRequest=serde_json::from_value(serde_json::json!({
            "game_id":"test","document_id":"pages","group_by_profile":true,"profile_override":"high",
            "selections":[{"element_id":"one","quantity":2},{"element_id":"six","quantity":3}]
        })).unwrap();
        let plan = planner::manual_plan(&f.game, doc, &InventoryFile::default(), &req).unwrap();
        let jobs = compile_jobs(doc, &plan, &f.printer, &f.root).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].source_pages, vec![1, 1, 6, 6, 6]);
        assert_eq!(doc.default_profile, "standard");
        assert!(doc.elements[0].profile.is_none());
        let mut plan = plan;
        plan.profile_override = Some("missing".into());
        plan.items.clear();
        assert!(
            compile_jobs(doc, &plan, &f.printer, &f.root)
                .unwrap_err()
                .to_string()
                .contains("unknown profile")
        );
    }
    #[test]
    fn replacements_merge_adjacent_pages_and_group_all_sets() {
        use crate::{model::InventoryFile, planner};
        let mut f = fixture(1);
        let template = f.game.documents[0].elements[0].clone();
        f.game.documents[0].elements = (1..=6)
            .map(|page| {
                let mut element = template.clone();
                element.id = format!("page-{page}");
                element.name = element.id.clone();
                element.page = page;
                element.profile = (page == 4).then(|| "high".into());
                element
            })
            .collect();
        f.printer
            .profiles
            .get_mut("high")
            .unwrap()
            .options
            .insert("PageSize".into(), "TA4".into());
        let doc = &f.game.documents[0];
        let mut req = production_request();
        req.selected_element_ids = doc.elements.iter().map(|e| e.id.clone()).collect();
        req.target_games = 1;
        req.group_by_profile = false;
        let inv = InventoryFile::default();
        let original = planner::production_plan(&f.game, doc, &inv, &req).unwrap();
        assert_eq!(
            compile_jobs(doc, &original, &f.printer, &f.root)
                .unwrap()
                .len(),
            3
        );
        req.profile_replacements
            .insert("standard".into(), "high".into());
        let plan = planner::production_plan(&f.game, doc, &inv, &req).unwrap();
        let jobs = compile_jobs(doc, &plan, &f.printer, &f.root).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].page_range(), "1-6");
        assert_eq!(jobs[0].options["PageSize"], "TA4");
        assert!(plan.requirements.iter().all(|r| r.profile == "high"));
        assert!(doc.elements[0].profile.is_none());
        req.group_by_profile = true;
        req.target_games = 3;
        let plan = planner::production_plan(&f.game, doc, &inv, &req).unwrap();
        let jobs = compile_jobs(doc, &plan, &f.printer, &f.root).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(
            jobs[0].source_pages,
            (0..3).flat_map(|_| 1..=6).collect::<Vec<_>>()
        );
    }

    #[test]
    fn replacements_are_single_pass_with_global_precedence_and_validation() {
        use crate::{model::*, planner};
        let f = fixture(1);
        let doc = &f.game.documents[0];
        let mut req: ManualPlanRequest = serde_json::from_value(serde_json::json!({
            "game_id":"test", "document_id":"pages", "group_by_profile":true,
            "profile_replacements":{"standard":"high", "high":"standard"},
            "selections":[{"element_id":"one","quantity":2},{"element_id":"three","quantity":1}]
        }))
        .unwrap();
        let inv = InventoryFile::default();
        let plan = planner::manual_plan(&f.game, doc, &inv, &req).unwrap();
        assert_eq!(plan.requirements[0].profile, "high");
        assert_eq!(plan.requirements[1].profile, "standard");
        let jobs = compile_jobs(doc, &plan, &f.printer, &f.root).unwrap();
        assert_eq!(jobs[0].source_pages, vec![1, 1]);
        assert_eq!(jobs[1].profile_id, "standard");
        req.profile_override = Some("standard".into());
        let plan = planner::manual_plan(&f.game, doc, &inv, &req).unwrap();
        assert!(plan.requirements.iter().all(|r| r.profile == "standard"));
        assert_eq!(
            compile_jobs(doc, &plan, &f.printer, &f.root).unwrap().len(),
            1
        );
        for replacements in [
            BTreeMap::from([("missing".into(), "high".into())]),
            BTreeMap::from([("standard".into(), "missing".into())]),
        ] {
            let mut invalid = plan.clone();
            invalid.items.clear();
            invalid.profile_replacements = replacements;
            assert!(
                compile_jobs(doc, &invalid, &f.printer, &f.root)
                    .unwrap_err()
                    .to_string()
                    .contains("unknown profile")
            );
        }
    }

    #[test]
    fn grouping_keeps_nup_sheet_counts_and_surplus() {
        use crate::{model::InventoryFile, planner};
        let f = fixture(8);
        let doc = &f.game.documents[0];
        let mut req = production_request();
        req.target_games = 10;
        req.selected_element_ids = vec!["one".into(), "six".into()];
        let mut inv = InventoryFile::default();
        inv.counts
            .insert("test".into(), BTreeMap::from([("one".into(), 3)]));
        let plan = planner::production_plan(&f.game, doc, &inv, &req).unwrap();
        let jobs = compile_jobs(doc, &plan, &f.printer, &f.root).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].source_pages, vec![1, 6, 6]);
        assert_eq!(plan.requirements[0].surplus, 1);
        assert_eq!(plan.requirements[1].surplus, 6);
    }
    #[test]
    fn assembled_pdf_preserves_page_content_inheritance_and_repetitions() {
        let f = fixture(1);
        let mut pdf = grouped_pdf(&f.root.join("source.pdf"), &[6, 1, 6, 3]).unwrap();
        pdf.save(f.root.join("grouped.pdf")).unwrap();
        let pdf = lopdf::Document::load(f.root.join("grouped.pdf")).unwrap();
        let pages = pdf.get_pages();
        assert_eq!(pages.len(), 4);
        assert!(pdf.catalog().unwrap().has(b"OutputIntents"));
        for ((_, id), expected) in pages.iter().zip([6, 1, 6, 3]) {
            assert_eq!(
                pdf.get_page_content(*id).unwrap(),
                format!("% page {expected}\n").as_bytes()
            );
            let page = pdf.get_dictionary(*id).unwrap();
            assert!(page.has(b"Resources"));
            assert!(page.has(b"MediaBox"));
            assert_eq!(page.get(b"Rotate").unwrap().as_i64().unwrap(), 90);
        }
        assert!(grouped_pdf(&f.root.join("source.pdf"), &[7]).is_err());
    }
}
