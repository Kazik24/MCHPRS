# Schematic files

`//load circuit.schem` first checks the schematic library's root. If the file isn't there, it searches subfolders. A single match loads; multiple matches ask you to specify a path. `//load rf/folder/circuit.schem` loads that exact file. Include `.schem` or `.schematic` in the filename.

The shared `schems/rf` library is read-only. Loading and copying its circuits are allowed, but `//save rf/circuit.schem` and saves into any of its subfolders are rejected before creating directories or overwriting files. Save a copy elsewhere, for example `//save my_builds/circuit.schem`. Docker also mounts the shared library read-only.

Paths must remain within the schematic library. Absolute paths, `..`, invalid extensions and links escaping the library are rejected. Links pointing into `rf` cannot bypass the save restriction. Recursive searching does not follow directory links, so a linked folder cannot create an endless search. Both slash styles work; `./rf/...` is accepted for loading and rejected for saving.

When Schemati is enabled, loading and recursive searching stay within the player's existing UUID folder. Saved files still use that namespace.

Filesystem tests cover recursive search, duplicate filenames, exact-path precedence, forbidden writes, preserved original files, traversal and linked directories. Client checks cover loading, denied saves and saving copies outside `rf`.
