<p align="center"><img src="static/paperjam-logo.svg" width="560" alt="Paperjam"></p>

# Paperjam

**Self-hosted PDF printing automation with reusable printer profiles, page-level quality controls, inventory tracking, and CUPS integration.**

Paperjam is a self-hosted web app for producing sets of printed materials. Choose a project, select its pages, enter the number of sets, and review a print plan. Paperjam works out what to print, accounts for pieces already in stock, and sends the jobs to CUPS.

It is useful for classroom packs, event materials, game kits, cards, and other small-batch products made from several PDF pages.

## Features

- **Reusable print profiles:** assign paper, color, borders, and quality settings to a document or an individual page.
- **Batch planning:** produce complete sets or replace a single missing piece.
- **Spare inventory:** browse stock by project, paper stack, and episode; edit counts across projects and save them together. Use existing stock and track surplus from pre-arranged sheets.
- **Page selection:** print only the pages listed in a project; omit covers or inserts sourced elsewhere.
- **Background runs:** follow progress and cancel queued work from the browser.
- **Dry-run mode:** review plans without sending jobs to a printer.

Paperjam prints existing PDFs. It does not create page layouts or arrange cards onto sheets. Each installation targets one CUPS queue, with profiles tailored to that queue's driver.

## Getting started

### Requirements

- Rust 1.90 or newer, or Docker with Compose.
- Your PDFs and a small set of TOML configuration files.
- For physical printing: a configured CUPS queue and CUPS client tools (`lp`, `lpstat`, and `cancel`). Linux is the simplest host; a remote CUPS server also works.

### 1. Create your data files

**The repository does not ship printer profiles, project definitions, or production PDFs.** Start with the copyable examples in [data/README.md](data/README.md). That guide walks through creating:

```text
data/
├── app.toml
├── printer.toml
├── games/
│   └── welcome-pack.toml
└── pdfs/
    └── welcome-pack.pdf
```

The example uses your own PDF with at least two pages. Keep the printer backend set to `dry-run` while setting up. Runtime inventory and history are created automatically.

### 2. Start Paperjam

From the repository root:

```bash
cargo run --locked
```

Open **http://localhost:8080**. Select your project, choose the pages and target number of sets, then click **Preview print plan**. Missing PDFs are reported in the UI; those documents cannot be printed until their files are available.

To keep PDFs elsewhere:

```bash
PAPERJAM_PDF_DIR=/path/to/your/pdfs cargo run --locked
```

### 3. Connect a printer

Install CUPS clients on the machine running Paperjam. On Ubuntu/Debian:

```bash
sudo apt install cups-client
lpstat -p -d
lpoptions -p YOUR_QUEUE -l
```

Set `destination` in `data/printer.toml` to your queue name. Use its supported option names and values when building profiles. If the printer is not configured yet, add it through your operating system's printer settings or the CUPS administration interface. See the [ET-8500 example below](#epson-ecotank-et-8500) for an Epson-specific setup.

Load the paper for a small sample run, then enable physical printing:

```bash
PAPERJAM_BACKEND=cups cargo run --locked --release
```

Starting the app does not print anything. **Start print run** submits the reviewed plan. Check a sample before running a batch, particularly after changing drivers, paper, or borderless settings. Paperjam does not automatically pause for paper changes; organize runs around the stock you have loaded.

## Grouping and one-run overrides

Use **Group printer jobs** to submit one CUPS job per print profile instead of page by page. Nonconsecutive pages and different required quantities are included exactly; existing inventory and N-up yield still determine how many sheets are needed.

- **By print profile** groups the whole selected PDF stack: three sets produce all three sets’ pages for the first profile in one job, followed by the next profile.
- **By print profile, per episode** groups each episode separately, in the game’s episode order. Within an episode the profile of its first page (normally the intro letter) prints first.
- **Separately for each game** splits every group per target set, so the output comes out already in set order. With per-episode grouping the order is episode, then set, then profile: all Episode 1 envelopes, set by set, then Episode 2. This means many more, smaller jobs. It has no effect on N-up documents or manual prints, which have no sets.

Within a group, profiles run in order of first appearance. Without **Separately for each game**, grouped output must be sorted into sets using the preview's packing guide.

Use **Print profile override** to apply a configured profile to the whole batch. Individual **Print** buttons also open a preview with this override. It replaces the full profile—including paper and borderless settings—for that run only; TOML assignments stay unchanged. The preview can toggle grouping and overrides before submission.

Use **Replace individual profiles** beside the grouping controls to map one profile to another for a run. For example, replacing Plain Standard with Plain Borderless Standard lets consecutive pages using either profile merge into one job. The preview offers the same controls and recalculates the job count immediately. Replacements match original page profiles once (they do not chain); the whole-run override takes priority. Changing the project or PDF clears the production replacements, and **Reset** clears them in one click. Saved TOML assignments are unchanged.

## Recovering a stalled print run

In **Print runs**, use **Clear stall and retry** on a running job. Paperjam cancels that run's current CUPS job, resubmits the same batch with its original pages, copies, and print settings, then continues the remaining work. Completed batches and other queued runs are kept; inventory is not consumed again. The confirmation explains that any pages already printed from the current batch may print again. If cancellation fails, the run fails visibly without submitting a duplicate. This does not restart CUPS or clear unrelated printer jobs.

Recovery checks: run `cargo test --locked`, then `cargo build --locked && python3 tests/stall_recovery.py`. The end-to-end check uses an isolated temporary app and fake CUPS commands; it does not contact a printer.

## Mixed portrait and landscape pages

Paperjam explicitly enables CUPS `pdfAutorotate=true` so landscape PDF pages rotate to fit the selected paper orientation instead of being cropped by a portrait request. This applies to individual pages and grouped jobs; the source PDFs are unchanged. A profile can set `pdfAutorotate = "false"` if fixed orientation is intentional.

Restart the app and create a fresh preview after updating; already submitted CUPS jobs retain their original options.

## Docker

Create the data files first using [the data guide](data/README.md), then run:

```bash
docker compose up --build -d
```

The image contains the app and CUPS clients. Configuration, project definitions, and PDFs are mounted at runtime; private production data is not baked into the image. Inventory and run history are stored in a persistent volume. The container uses its own `/app/data/pdfs` and `/app/data/state` paths, so host-specific paths in `app.toml` do not need to change.

To use an external PDF directory and a remote CUPS server:

```bash
PAPERJAM_PDF_DIR=/srv/print-pdfs \
PAPERJAM_CUPS_SERVER=print-server.local:631 \
docker compose up --build -d
```

Install printer drivers on the **CUPS server**, not in the Paperjam container. The server must allow connections from the container. Compose defaults to dry-run; set `PAPERJAM_BACKEND=cups` when ready. Avoid `docker compose down -v` if you want to retain inventory and history.

## Configuration

Files are loaded at startup. Restart after changing profiles or project definitions.

| Environment variable | Overrides |
| --- | --- |
| `PAPERJAM_BIND` | HTTP listen address |
| `PAPERJAM_PDF_DIR` | PDF root directory |
| `PAPERJAM_STATE_DIR` | Inventory and history directory |
| `PAPERJAM_BACKEND` | `dry-run` or `cups` |
| `PAPERJAM_CUPS_SERVER` | CUPS server host and optional port |
| `PAPERJAM_DESTINATION` | Printer queue name |

Environment variables take precedence over TOML. Older `GAMEPRINTER_*` names are accepted as fallbacks. These overrides do not remove the need to create the configuration files.

Paperjam has no built-in authentication. Use a trusted network or an authenticated reverse proxy. The local setup example binds to `127.0.0.1:8080`; Compose publishes port 8080 on localhost.

## Epson EcoTank ET-8500

This optional setup uses Epson's **ESC/P-R2** driver, which exposes paper-specific quality choices including Ultra Glossy–Max Quality. Paperjam also works with other CUPS drivers; their profile option names may differ.

### Install the driver

On Ubuntu 24.04 amd64:

```bash
sudo apt update
sudo apt install cups cups-client libcupsimage2t64
sudo systemctl enable --now cups
```

Download **Epson Inkjet Printer Driver 2 (ESC/P-R) for Linux** for the ET-8500 from the [Epson Download Center](https://download-center.epson.com/). Choose a package for your architecture, then install it. Replace the placeholder below with the actual downloaded filename:

```bash
sudo apt install ./EPSON_DRIVER_PACKAGE.deb
lpinfo -m | grep 'ET-8500'
```

On older Ubuntu/Debian releases, the CUPS image library may be named `libcupsimage2`. Other distributions need their corresponding package format. Refer to [Epson's installation manual](https://download.ebz.epson.net/man/linux/escpr2.html). Under WSL, starting CUPS with `systemctl` requires systemd.

### Create a queue

Replace `PRINTER_HOST` with your printer's IP address or hostname, and `MODEL_ID` with the full identifier from `lpinfo -m`:

```bash
sudo lpadmin -p ET8500_ESCPR2 -E \
  -v ipp://PRINTER_HOST:631/ipp/print \
  -m 'MODEL_ID'
lpoptions -p ET8500_ESCPR2 -l
```

`ET8500_ESCPR2` is an example queue name; use the same name in your `printer.toml`. CUPS 2.x may display a deprecation warning for traditional PPD-based drivers.

### Configure paper and quality

The following mappings were checked against the ET-8500 definition in ESC/P-R2 1.2.42-1. Verify them against your installed queue before use:

| Intended setting | `PageSize` | `MediaType` |
| --- | --- | --- |
| Plain paper, Standard | `A4` | `PLAIN_DRAFT` |
| Plain paper, Standard, borderless | `TA4` | `PLAIN_DRAFT` |
| Plain paper, High, borderless | `TA4` | `PLAIN_NORMAL` |
| Epson Ultra Glossy, Max Quality | `A4` | `PLATINA_HIGH` |

In this model's driver, `PLAIN_DRAFT` is the internal name for **Standard**, and `PLAIN_NORMAL` is **High**. Use the displayed labels rather than interpreting the suffixes. The [data guide](data/README.md#optional-epson-et-8500-profiles) contains a complete Epson configuration example.

Do not mix ESC/P-R2 settings with IPP Everywhere options such as `cupsPrintQuality` or `A4.Borderless`. A Linux quality label does not guarantee identical output to a Windows driver; compare representative samples if matching existing prints matters.

If a job reports “Filter failed”, check the driver's runtime libraries:

```bash
ldd /opt/epson-inkjet-printer-escpr2/cups/lib/filter/epson-escpr-wrapper2
```

A missing `libcupsimage.so.2` is supplied by `libcupsimage2t64` on Ubuntu 24.04. Install missing dependencies before retrying.

## Development

```bash
cargo fmt --all -- --check
cargo test --locked
cargo build --locked --release
```

Tests use fixtures and do not require private profiles, PDFs, or a connected printer. Web assets are embedded in the Rust binary; rebuild after editing `static/`.

## Proudly used by CaseClosed

<a href="https://www.caseclosed.no/"><img src="static/caseclosed-logo.png" width="160" alt="Caseclosed logo"></a>

Proudly used by **[CaseClosed](https://www.caseclosed.no/) in Norway** and **[Caseclosed Mysteries](https://www.caseclosedmysteries.com/)** internationally for their printed murder mystery materials.