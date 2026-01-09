# RustFileFinder

RustFileFinder è un'applicazione a riga di comando scritta in Rust per la ricerca ricorsiva di file in base al nome e/o al contenuto. Supporta file di testo e documenti PDF, esecuzione parallela, preset configurabili e formati di output strutturati per dimostrazioni e valutazione accademica.

## Contesto Accademico

- Corso: Linguaggi di Programmazione
- Università: Università degli Studi Roma Tre
- Anno Accademico: 2025–2026
- Docente: Flavio Lombardi

## Funzionalità

- Attraversamento ricorsivo delle directory (WalkDir)
- Ricerca opzionale per nome file (case-insensitive)
- Ricerca nel contenuto tramite espressioni regolari (crate regex)
- Analisi parallela dei file (Rayon)
- Scansione di file di testo con limite di dimensione (max_bytes)
- Ricerca opzionale nel contenuto dei PDF tramite estrazione del testo (pdf-extract)
- Preset e valori di default configurabili tramite file TOML
- Formati di output: JSON o Markdown
- Modalità verbose per il debug dei PDF non leggibili
- Isolamento dei panic per file (catch_unwind)

## Struttura del Progetto

    rustfilefinder/
    ├── samples/              # File di testo di esempio (txt, md)
    ├── samples_pdf/          # File PDF di esempio
    ├── src/
    │   └── main.rs           # Codice sorgente dell'applicazione
    ├── Cargo.toml            # Dipendenze
    ├── rustfilefinder.toml   # Configurazione dei preset
    └── README.md             # Documentazione

## Requisiti

- Toolchain Rust (edition 2021)
- Cargo package manager

Tutte le dipendenze sono definite in Cargo.toml.

## Compilazione

    cargo build

## Utilizzo

Mostrare l'help generale:

    cargo run -- --help

Mostrare l'help del comando search:

    cargo run -- search --help

### Esecuzione tramite preset (consigliato)

Preset demo per file di testo (samples/):

    cargo run -- search --preset demo_text

Preset demo per file PDF (samples_pdf/):

    cargo run -- search --preset demo_pdf

Abilitare output verbose (stampa PDF non leggibili):

    cargo run -- search --preset demo_pdf --verbose

### Esecuzioni manuali (senza preset)

Ricerca nel contenuto di file di testo:

    cargo run -- search --dir samples --content "(?i)compilatore|interprete" --ext "txt,md" --format json

Ricerca nel contenuto di file PDF (richiede --include-pdf):

    cargo run -- search --dir samples_pdf --include-pdf --content "(?i)compilatore|interprete" --ext "pdf" --format json

Ricerca solo per nome file:

    cargo run -- search --dir . --name "report" --format json

Ricerca combinata (nome + contenuto):

    cargo run -- search --dir . --name "lezione" --content "(?i)semantica|tipi" --ext "txt,md,pdf" --include-pdf --format json

Limitare il numero di risultati stampati (utile per demo):

    cargo run -- search --preset demo_text --limit 5

## Configurazione (rustfilefinder.toml)

RustFileFinder può caricare preset e valori di default da un file di configurazione TOML.

Preset attualmente definiti:

    [presets.demo_text]
    dir = "samples"
    include_pdf = false
    ext = "txt,md"
    content = "(?i)compilatore|interprete|semantica|tipi|rust|python"
    format = "json"

    [presets.demo_pdf]
    dir = "samples_pdf"
    include_pdf = true
    ext = "pdf"
    content = "(?i)compilatore|interprete|semantica|tipi|rust|python"
    format = "json"

### Elenco dei preset disponibili

    cargo run -- presets

## Output

Sono supportati due formati di output:

- JSON: output strutturato leggibile da macchine
- Markdown: report leggibile da esseri umani

Ogni esecuzione riporta:

- files_discovered
- files_scanned_text / files_scanned_pdf
- contatori dei file scartati (non-text, troppo grandi, non UTF-8, non leggibili)
- matches_total / matches_printed
- elapsed_ms
- risultati (path, matched_name, matched_content, snippet)

## Note sul Supporto PDF

- La ricerca nei PDF viene eseguita solo se include_pdf è abilitato.
- Alcuni PDF possono non essere leggibili a causa di cifratura o limiti dell'estrazione del testo.
- Usare --verbose per identificare i PDF scartati.

## Licenza

Progetto accademico sviluppato per il corso di Linguaggi di Programmazione. L'utilizzo è soggetto alle politiche del corso.

_______________________________________________

English Version

# RustFileFinder

RustFileFinder is a command-line application written in Rust for recursively searching files by filename and/or file content. It supports both text files and PDF documents, provides parallel processing, configurable presets, and structured output formats for demos and evaluation.

## Academic Context

- Course: Linguaggi di Programmazione
- University: Roma Tre University
- Academic Year: 2025–2026
- Instructor: Flavio Lombardi

## Features

- Recursive directory traversal (WalkDir)
- Optional filename matching (case-insensitive)
- Optional content search using regular expressions (regex crate)
- Parallel scanning using Rayon
- Text scanning with size limit (max_bytes)
- Optional PDF content search via text extraction (pdf-extract)
- Presets and defaults loaded from a TOML config file
- Output format: JSON or Markdown
- Verbose mode for debugging unreadable PDFs
- Panic isolation for per-file processing (catch_unwind)

## Project Structure

    rustfilefinder/
    ├── samples/              # Example text files (txt, md)
    ├── samples_pdf/          # Example PDF files
    ├── src/
    │   └── main.rs           # Application source code
    ├── Cargo.toml            # Dependencies
    ├── rustfilefinder.toml   # Preset configuration
    └── README.md             # Documentation

## Requirements

- Rust toolchain (edition 2021)
- Cargo package manager

All dependencies are defined in Cargo.toml.

## Build

    cargo build

## Usage

Show general help:

    cargo run -- --help

Show help for the search subcommand:

    cargo run -- search --help

### Preset-based runs (recommended)

Run the demo preset for text files (samples/):

    cargo run -- search --preset demo_text

Run the demo preset for PDF files (samples_pdf/):

    cargo run -- search --preset demo_pdf

Enable verbose debug output (prints unreadable PDFs):

    cargo run -- search --preset demo_pdf --verbose

### Manual runs (without presets)

Search inside text files in a folder:

    cargo run -- search --dir samples --content "(?i)compilatore|interprete" --ext "txt,md" --format json

Search inside PDF files (must enable PDF search explicitly):

    cargo run -- search --dir samples_pdf --include-pdf --content "(?i)compilatore|interprete" --ext "pdf" --format json

Search by filename only:

    cargo run -- search --dir . --name "report" --format json

Combine name + content:

    cargo run -- search --dir . --name "lezione" --content "(?i)semantica|tipi" --ext "txt,md,pdf" --include-pdf --format json

Limit printed matches (useful for demos):

    cargo run -- search --preset demo_text --limit 5

## Configuration (rustfilefinder.toml)

RustFileFinder can load defaults and presets from a TOML configuration file.

Your current presets:

    [presets.demo_text]
    dir = "samples"
    include_pdf = false
    ext = "txt,md"
    content = "(?i)compilatore|interprete|semantica|tipi|rust|python"
    format = "json"

    [presets.demo_pdf]
    dir = "samples_pdf"
    include_pdf = true
    ext = "pdf"
    content = "(?i)compilatore|interprete|semantica|tipi|rust|python"
    format = "json"

### Presets listing

List available presets (reads from rustfilefinder.toml if present):

    cargo run -- presets

## Output

Two formats are supported:

- JSON: machine-readable output containing run statistics and results
- Markdown: human-readable report

Each run prints:

- files_discovered
- files_scanned_text / files_scanned_pdf
- skipped counters (non-text, too large, non-UTF8, unreadable)
- matches_total / matches_printed
- elapsed_ms
- results (path, matched_name, matched_content, snippet)

## Notes on PDF Support

- PDF search is only performed when include_pdf is enabled.
- Some PDFs may be unreadable due to encryption, malformed structure, or extraction limitations.
- Use --verbose to print which PDFs were skipped as unreadable.

## License

Academic project (coursework). Redistribution terms depend on the course policy.
