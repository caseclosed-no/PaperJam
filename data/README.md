# Set up your Paperjam data

This folder is used to configure your printing options.
Create the configuration and project files below for your own printer and PDFs. Paperjam reads them from the repository root at startup.

## 1. Create the directories

```bash
mkdir -p data/games data/pdfs
```

Your completed setup will look like this:

```text
data/
├── README.md
├── app.toml                 # App settings
├── printer.toml             # Queue and print profiles
├── games/                   # One TOML file per project
│   └── welcome-pack.toml
├── pdfs/                    # Your source PDFs
│   └── welcome-pack.pdf
└── state/                   # Created by Paperjam; do not create files manually
```

The folder name `games` is part of the current schema. It holds any kind of project, not just games.

## 2. Create `data/app.toml`

```toml
bind = "127.0.0.1:8080"
pdf_directory = "data/pdfs"
state_directory = "data/state"
```

Paths are relative to the working directory unless absolute. Use `PAPERJAM_PDF_DIR` to keep source PDFs outside the repository. Paperjam creates the state directory and manages its files automatically.

## 3. Create `data/printer.toml`

This minimal configuration works for dry-run planning without a printer. Replace `YOUR_QUEUE` with a real CUPS queue name before enabling physical printing:

```toml
backend = "dry-run"
destination = "YOUR_QUEUE"
poll_interval_ms = 1000

[default_options]
job-sheets = "none"
sides = "one-sided"
number-up = "1"

[profiles.standard]
name = "Standard"
note = "Use the queue's default paper and quality settings."
media = "plain"

[profiles.standard.options]
```

`standard` is a profile ID of your choosing. `name`, `note`, and `media` describe the profile; driver options go in its `options` table. An empty options table uses queue defaults plus `default_options`. Profile options override entries with the same name in `default_options`.

List your queue's choices with:

```bash
lpoptions -p YOUR_QUEUE -l
```

Each key/value pair becomes an `lp -o key=value` argument. Copy the driver-specific names, including capitalization; options that work for one driver may not work for another. A descriptive `media` value alone does not configure paper handling.

For a remote CUPS server, add `server = "print-server.local:631"` beside `destination`, before any table headers, or set `PAPERJAM_CUPS_SERVER`.

## 4. Add a PDF and project

Put your own PDF containing at least two pages at `data/pdfs/welcome-pack.pdf`. Create `data/games/welcome-pack.toml`:

```toml
id = "welcome-pack"
name = "Welcome pack"

[[episodes]]
id = "main"
name = "Handouts"

[[documents]]
id = "handouts"
name = "Welcome handouts"
file = "welcome-pack.pdf"
default_profile = "standard"
copies_per_page = 1

[[documents.elements]]
id = "welcome-letter"
name = "Welcome letter"
page = 1
episode = "main"

[[documents.elements]]
id = "venue-map"
name = "Venue map"
page = 2
episode = "main"
```

Change the filenames, names, and page numbers to match your material. The example references only pages 1 and 2; any additional PDF pages are omitted from production.

| Field | Meaning |
| --- | --- |
| Project `id` | Unique, stable project identifier |
| `episodes` | Named sections used to group elements in the UI |
| `documents` | PDFs, usually separated by physical paper stack |
| `file` | Relative PDF path beneath the configured PDF root |
| `default_profile` | ID of a profile in `printer.toml` |
| `copies_per_page` | Finished copies of an element obtained from one sheet |
| `elements` | Named, individually tracked pages to produce |
| Element `episode` | ID of a section declared in this project |
| Element `page` | Actual one-based page number in that PDF |

List only pages you intend to print. Page numbers must be unique within each document and within the PDF's range. Keep the original PDF numbering when omitting a page. Project IDs must be unique across files; document IDs within a project, and element IDs across the whole project, must also be unique.

Keep existing element IDs stable when rearranging pages: inventory uses those IDs. Avoid reusing an old ID for a different piece.

## 5. Preview a run

From the repository root:

```bash
cargo run --locked
```

Open **http://localhost:8080**, select your project, and preview a batch. Check quantities and profile assignments. In dry-run mode, starting a run simulates printing; stock-aware runs still update inventory, so use **Print everything** for a trial that should not consume existing stock.

For Docker, create the same files before running `docker compose up --build -d`. Compose mounts `app.toml`, `printer.toml`, and `games/` read-only, and mounts PDFs separately. It overrides the app's PDF and state paths with container paths.

## Add profiles and per-page overrides

Add another `[profiles.PROFILE_ID]` table and its `[profiles.PROFILE_ID.options]` table to `printer.toml`. After choosing supported driver options, assign the new profile to a document with `default_profile`, or to a single element with:

```toml
profile = "YOUR_PROFILE_ID"
```

Place that line inside the existing `[[documents.elements]]` entry. Every referenced profile must exist. Restart Paperjam after changes.

## Optional Epson ET-8500 profiles

First follow the [Epson driver installation instructions](../README.md#epson-ecotank-et-8500). If you use that ESC/P-R2 queue, you can **replace** the minimal `printer.toml` above with this configuration. Keep `standard` as the default in the example project; use the other IDs for exceptions:

```toml
backend = "dry-run"
destination = "ET8500_ESCPR2"
poll_interval_ms = 1000

[default_options]
job-sheets = "none"
sides = "one-sided"
Duplex = "None"
Ink = "COLOR"
InputSlot = "Auto"
orientation-requested = "3"
pdfAutorotate = "true"
number-up = "1"
Collate = "True"

[profiles.standard]
name = "Plain Standard"
media = "plain"
[profiles.standard.options]
PageSize = "A4"
MediaType = "PLAIN_DRAFT"

[profiles.borderless]
name = "Plain Borderless Standard"
media = "plain"
[profiles.borderless.options]
PageSize = "TA4"
MediaType = "PLAIN_DRAFT"

[profiles.borderless-high]
name = "Plain Borderless High"
media = "plain"
[profiles.borderless-high.options]
PageSize = "TA4"
MediaType = "PLAIN_NORMAL"

[profiles.photo-max]
name = "Ultra Glossy Max Quality"
media = "ultra-glossy"
[profiles.photo-max.options]
PageSize = "A4"
MediaType = "PLATINA_HIGH"
```

These mappings are specific to the ET-8500 ESC/P-R2 driver. Its `PLAIN_DRAFT` choice is labeled **Plain paper–Standard**, `PLAIN_NORMAL` is **Plain paper–High**, and `PLATINA_HIGH` is **Epson Ultra Glossy–Max Quality**. Confirm the choices with `lpoptions -p ET8500_ESCPR2 -l`.

Auto-rotation fits landscape pages onto the chosen sheet orientation. It is enabled by Paperjam by default, including when a portrait orientation is explicitly requested.

Photo Max uses A4 with borders. Borderless A4 is `TA4`. Load the matching stock before printing. If required for your output tray, add `outputorder = "reverse"` to `default_options`; reversal applies within each submitted job, not across an entire run.

## Pre-arranged sheets

For a PDF page containing eight copies of the same card, set `copies_per_page = 8`. A request for ten finished cards needs two sheets and produces six surplus cards. Keep driver `number-up=1`: the PDF already contains the layout.

This model assumes each sheet repeats one tracked element. A sheet containing eight different pieces cannot be tracked as eight independent elements through `copies_per_page`.

## Inventory, updates, and backups

Inventory counts finished pieces, not sheets. Stock-aware runs deduct available inventory at the start. After a failure or cancellation, inspect the physical output before restoring consumed stock. Do not edit state files while Paperjam is running.

When replacing a PDF, update page numbers and review profile assignments, while preserving IDs for existing elements. Preview a dry run before producing a batch. Back up configuration, project files, PDFs, and the state directory together.

To enable physical printing after checking your setup:

```bash
PAPERJAM_BACKEND=cups cargo run --locked --release
```

Use one representative page for an initial sample. Paperjam does not load paper or automatically pause for stock changes.
