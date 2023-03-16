## 0.2.0

* Added `VersionSpecifiers`, a thin wrapper around `Vec<VersionSpecifier>` with a serde implementation. `VersionSpecifiers::from_str` is now preferred over `parse_version_specifiers`.
* Reexport rust function for python module