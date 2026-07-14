# Build fix 1.1.1

This revision fixes the Windows build errors reported by GitHub Actions:

- Win32 handle conversion for `windows 0.58`;
- null Win32 handles now use pointer-safe checks;
- `GetCursorPos` result handling;
- mutable/immutable borrow conflict in text rendering;
- removed unused variables and imports;
- dependency versions pinned for reproducible builds.

Run `cargo build --release` or use the included GitHub Actions workflow.
