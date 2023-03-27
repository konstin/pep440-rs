## Unreleased

 * Added `Version::from_release`, a constructor for a version that is just a release such as `3.8`.

## 0.2.0

* Added `VersionSpecifiers`, a thin wrapper around `Vec<VersionSpecifier>` with a serde implementation. `VersionSpecifiers::from_str` is now preferred over `parse_version_specifiers`.
* Reexport rust function for python module