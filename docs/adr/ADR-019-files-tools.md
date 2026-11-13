# ADR-019: Files tools (Large & Old, Duplicates, Similar Images, Downloads, AI clutter)

- **Status:** Accepted, by the product owner's decision. It is the first time Kleari reads the
  contents of the user's files, locally and only to compare them.
- **Context:** People asked for the finders other Mac cleaners have: large and old files,
  duplicates, similar photos, a downloads manager by source app, and AI clutter (model
  downloads, AI apps' caches and logs).
- **Decision:**
  - **One walk** (`files/walk.rs`) of the home folder, read-only, through handles without
    following symlinks, never entering other volumes, the Library, the Trash, packages,
    `node_modules`, `.git`, private dotfiles or folders excluded in Settings; bounded to 3
    million files.
  - **Large & Old:** files from a size you choose (100 MB to 5 GB), and files of 50 MB or more
    not opened or changed for a time you choose (3 months to 2 years).
  - **Duplicates:** files of 1 MB or more with the same size, then the same SHA-256 of their
    first 64 KB, then of all of it. Hard links aren't copies. Contents are read only to hash,
    and hashes are never stored. Each file is opened only if it is still the file the walk saw.
  - **Similar Images:** up to 20,000 images (JPEG, PNG, HEIC, TIFF, WebP, RAW) are decoded by
    macOS (ImageIO through `NSImage`, as Quick Look does) to a 9×8 grayscale thumbnail, whose
    difference hash groups images within 5 bits of each other. Pixels never leave the function.
  - **Downloads:** what is directly in Downloads, grouped by the app named in the
    `com.apple.quarantine` tag macOS adds to downloads. Only the app's name is read, never the
    address it came from.
  - **AI clutter:** fixed places AI tools use (Ollama, Hugging Face, LM Studio, GPT4All,
    Whisper, PyTorch, Keras; ChatGPT, Claude, Cursor and Copilot caches and logs), each model a
    row where one can be removed alone; Ollama's models only as a whole, with `ollama rm` for
    one.
  - **Choosing** sends items to the review as `Kind::UserChosen` (ADR-018), so the same bounds,
    open-in-an-app hold and quarantine apply. In a group of copies or look-alikes one is always
    kept: the page doesn't let you choose it, and Rust holds it back if every member is sent.
- **Consequences:** Kleari reads file contents for Duplicates and Similar Images, on this Mac,
  only when you run those tools. Privacy and the threat model say so. APFS clones of one file
  are listed as copies though removing one frees little; the page says sizes are what each
  file takes.
