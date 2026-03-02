# Batch Rename

Rename multiple files at once with pattern-based transformations.

## Opening

Select two or more files and press **F2** (or **Cmd+R**). The Batch Rename dialog opens with a live preview of all changes.

## Pattern Controls

All controls update the preview table in real time.

### Find & Replace

Enter text in **Find** and **Replace** fields to perform a global string replacement on each filename (stem only, extension is preserved). Check **Regex** to use a regular expression in the Find field.

### Prefix & Suffix

Add text before or after the filename stem. For example, prefix `backup_` turns `report.pdf` into `backup_report.pdf`.

### Numbering

Enable **Numbering** to append a sequential number to each filename. Configure:
- **Start** — first number in the sequence (default: 1)
- **Step** — increment between files (default: 1)
- **Digits** — zero-padded width (default: 2, so `01`, `02`, ...)

### Case Transform

Apply a case transformation to the filename stem:
- **None** — no change
- **UPPER CASE** — all uppercase
- **lower case** — all lowercase
- **Title Case** — capitalize first letter of each word

## Preview Table

The table shows two columns: **Original Name** and **New Name**. Changed names are highlighted in green. Conflicts (duplicate names or names containing `/`) are highlighted in red.

Each row's new name is editable — click it to override the pattern for that specific file.

## Applying

Click **Apply** to rename all changed files. A progress bar shows the current file and count. Click **Cancel** during the operation to stop after the current file.

When done, a summary shows how many files were renamed and lists any errors.

## Supported Backends

Batch rename works on local files, S3 objects, and SFTP files.
