use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use lopdf::Document as PdfDocument;

use crate::model::{AppConfig, Game, PrinterConfig};

// Paperjam names take precedence; existing deployments keep working.
fn env_override(suffix: &str) -> Result<String, std::env::VarError> {
    std::env::var(format!("PAPERJAM_{suffix}"))
        .or_else(|_| std::env::var(format!("GAMEPRINTER_{suffix}")))
}

pub fn load_app_config(path: impl AsRef<Path>) -> anyhow::Result<AppConfig> {
    let mut config: AppConfig = load_toml(path.as_ref())?;
    if let Ok(value) = env_override("BIND") {
        config.bind = value;
    }
    if let Ok(value) = env_override("PDF_DIR") {
        config.pdf_directory = value;
    }
    if let Ok(value) = env_override("STATE_DIR") {
        config.state_directory = value;
    }
    Ok(config)
}

pub fn load_games(directory: impl AsRef<Path>) -> anyhow::Result<Vec<Game>> {
    let directory = directory.as_ref();
    let mut games = Vec::new();
    for entry in
        fs::read_dir(directory).with_context(|| format!("reading {}", directory.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|x| x.to_str()) == Some("toml") {
            games.push(load_toml(&path)?);
        }
    }
    games.sort_by(|a: &Game, b| a.name.cmp(&b.name));
    Ok(games)
}

pub fn load_printer_config(path: impl AsRef<Path>) -> anyhow::Result<PrinterConfig> {
    let mut config: PrinterConfig = load_toml(path.as_ref())?;
    if let Ok(value) = env_override("BACKEND") {
        config.backend = value;
    }
    if let Ok(value) = env_override("CUPS_SERVER") {
        config.server = if value.trim().is_empty() {
            None
        } else {
            Some(value)
        };
    }
    if let Ok(value) = env_override("DESTINATION") {
        config.destination = value;
    }
    Ok(config)
}

fn load_toml<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let source = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&source).with_context(|| format!("parsing {}", path.display()))
}

pub fn validate(
    games: &[Game],
    printer: &PrinterConfig,
    pdf_root: &Path,
) -> anyhow::Result<Vec<String>> {
    if games.is_empty() {
        bail!("no game definitions found");
    }
    if printer.backend != "cups" && printer.backend != "dry-run" {
        bail!("printer backend must be 'cups' or 'dry-run'");
    }
    let mut warnings = Vec::new();
    let mut game_ids = HashSet::new();
    for game in games {
        if !game_ids.insert(&game.id) {
            bail!("duplicate game id '{}'", game.id);
        }
        let episode_ids: HashSet<_> = game.episodes.iter().map(|e| e.id.as_str()).collect();
        if episode_ids.len() != game.episodes.len() {
            bail!("game '{}' has duplicate episode IDs", game.id);
        }
        let mut document_ids = HashSet::new();
        let mut element_ids = HashSet::new();
        for document in &game.documents {
            if !document_ids.insert(&document.id) {
                bail!(
                    "game '{}' has duplicate document '{}'",
                    game.id,
                    document.id
                );
            }
            if document.copies_per_page == 0 {
                bail!(
                    "document '{} / {}' has copies_per_page=0",
                    game.id,
                    document.id
                );
            }
            if !printer.profiles.contains_key(&document.default_profile) {
                bail!(
                    "document '{} / {}' uses unknown profile '{}'",
                    game.id,
                    document.id,
                    document.default_profile
                );
            }
            let mut pages = HashSet::new();
            for element in &document.elements {
                if element.page == 0 {
                    bail!("element '{}' has page 0; PDF pages start at 1", element.id);
                }
                if !element_ids.insert(&element.id) {
                    bail!(
                        "game '{}' has duplicate element id '{}'",
                        game.id,
                        element.id
                    );
                }
                if !pages.insert(element.page) {
                    bail!(
                        "document '{} / {}' maps PDF page {} more than once",
                        game.id,
                        document.id,
                        element.page
                    );
                }
                if !episode_ids.contains(element.episode.as_str()) {
                    bail!(
                        "element '{}' uses unknown episode '{}'",
                        element.id,
                        element.episode
                    );
                }
                if let Some(profile) = &element.profile
                    && !printer.profiles.contains_key(profile)
                {
                    bail!(
                        "element '{}' uses unknown profile '{}'",
                        element.id,
                        profile
                    );
                }
            }
            let pdf_path = safe_pdf_path(pdf_root, &document.file)?;
            if !pdf_path.exists() {
                warnings.push(format!(
                    "{} / {} unavailable: missing {}",
                    game.name,
                    document.name,
                    pdf_path.display()
                ));
                continue;
            }
            let pdf = PdfDocument::load(&pdf_path)
                .with_context(|| format!("reading PDF {}", pdf_path.display()))?;
            let page_count = pdf.get_pages().len() as u32;
            if let Some(page) = pages
                .iter()
                .filter(|&&page| page == 0 || page > page_count)
                .min()
            {
                bail!(
                    "document '{} / {}' references page {} outside PDF range 1..={}",
                    game.id,
                    document.id,
                    page,
                    page_count
                );
            }
        }
    }
    Ok(warnings)
}

pub fn safe_pdf_path(root: &Path, file: &str) -> anyhow::Result<PathBuf> {
    let relative = Path::new(file);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("PDF file path must be relative and cannot contain '..': {file}");
    }
    Ok(root.join(relative))
}

pub fn find_game_document<'a>(
    games: &'a [Game],
    game_id: &str,
    document_id: &str,
) -> anyhow::Result<(&'a Game, &'a crate::model::Document)> {
    let game = games
        .iter()
        .find(|g| g.id == game_id)
        .with_context(|| format!("unknown game '{game_id}'"))?;
    let document = game
        .documents
        .iter()
        .find(|d| d.id == document_id)
        .with_context(|| format!("unknown document '{document_id}'"))?;
    Ok((game, document))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_pdf_pages_allow_gaps_but_reject_invalid_and_duplicate_pages() {
        use lopdf::{Object, dictionary};
        let root = std::env::temp_dir().join(format!("paperjam-pages-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mut pdf = PdfDocument::with_version("1.5");
        let pages_id = pdf.new_object_id();
        let kids: Vec<Object> = (0..3)
            .map(|_| {
                let id = pdf.add_object(dictionary! {
                    "Type" => "Page", "Parent" => pages_id,
                    "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()]
                });
                Object::Reference(id)
            })
            .collect();
        pdf.objects.insert(
            pages_id,
            dictionary! {"Type" => "Pages", "Kids" => kids, "Count" => 3}.into(),
        );
        let catalog = pdf.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id});
        pdf.trailer.set("Root", catalog);
        pdf.save(root.join("subset.pdf")).unwrap();
        let mut game: Game = toml::from_str(
            r#"
            id = "sample"
            name = "Sample"
            [[episodes]]
            id = "main"
            name = "Main"
            [[documents]]
            id = "pages"
            name = "Selected pages"
            file = "subset.pdf"
            default_profile = "white-normal"
            [[documents.elements]]
            id = "insert"
            name = "Insert"
            page = 2
            episode = "main"
        "#,
        )
        .unwrap();
        let printer: PrinterConfig = toml::from_str(
            r#"
            backend = "dry-run"
            destination = "test-queue"
            [profiles.white-normal]
            name = "Standard"
        "#,
        )
        .unwrap();
        assert!(
            validate(&[game.clone()], &printer, &root)
                .unwrap()
                .is_empty()
        );
        game.documents[0].elements[0].page = 4;
        assert!(
            validate(&[game.clone()], &printer, &root)
                .unwrap_err()
                .to_string()
                .contains("outside PDF range")
        );
        game.documents[0].elements[0].page = 0;
        assert!(
            validate(&[game.clone()], &printer, &root)
                .unwrap_err()
                .to_string()
                .contains("page 0")
        );
        game.documents[0].elements[0].page = 2;
        let mut duplicate = game.documents[0].elements[0].clone();
        duplicate.id = "duplicate".into();
        game.documents[0].elements.push(duplicate);
        assert!(
            validate(&[game], &printer, &root)
                .unwrap_err()
                .to_string()
                .contains("more than once")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
