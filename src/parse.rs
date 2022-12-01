//! Parses PEP 440 versions and version specifiers

use crate::{LocalSegment, Operator, Pep440Error, PreRelease, Version, VersionSpecifier};
use lazy_static::lazy_static;
use regex::{Captures, Regex};
use std::str::FromStr;
use unicode_width::UnicodeWidthStr;

/// A regex copied from <https://peps.python.org/pep-0440/#appendix-b-parsing-version-strings-with-regular-expressions>,
/// updated to support stars for version ranges
const VERSION_RE_INNER: &str = r#"
(?:
    (?:v?)                                            # <https://peps.python.org/pep-0440/#preceding-v-character>
    (?:(?P<epoch>[0-9]+)!)?                           # epoch
    (?P<release>[0-9*]+(?:\.[0-9*]+)*)                # release segment, this now allows for * versions which are more lenient than necessary so we can put better error messages in the code
    (?P<pre_field>                                    # pre-release
        [-_\.]?
        (?P<pre_name>(a|b|c|rc|alpha|beta|pre|preview))
        [-_\.]?
        (?P<pre>[0-9]+)?
    )?
    (?P<post_field>                                   # post release
        (?:-(?P<post_old>[0-9]+))
        |
        (?:
            [-_\.]?
            (?P<post_l>post|rev|r)
            [-_\.]?
            (?P<post_new>[0-9]+)?
        )
    )?
    (?P<dev_field>                                    # dev release
        [-_\.]?
        (?P<dev_l>dev)
        [-_\.]?
        (?P<dev>[0-9]+)?
    )?
)
(?:\+(?P<local>[a-z0-9]+(?:[-_\.][a-z0-9]+)*))?       # local version
"#;

lazy_static! {
    /// Matches a python version, such as `1.19.a1`. Based on the PEP 440 regex
    static ref VERSION_RE: Regex = Regex::new(&format!(
        r#"(?xi)^(?:\s*){}(?:\s*)$"#, VERSION_RE_INNER
    )).unwrap();

    /// Matches a python version specifier, such as `>=1.19.a1` or `4.1.*`. Extends the PEP 440 regex
    static ref VERSION_SPECIFIER_RE: Regex = Regex::new(&format!(
        r#"(?xi)^(?:\s*)(?P<operator>(~=|==|!=|<=|>=|<|>|===))(?:\s*){}(?:\s*)$"#,
        VERSION_RE_INNER
    )).unwrap();
}

/// Extract for reusability around star/non-star
#[allow(clippy::type_complexity)]
fn parse_version_modifier(
    captures: &Captures,
) -> Result<
    (
        usize,
        Option<(PreRelease, usize)>,
        Option<usize>,
        Option<usize>,
        Option<Vec<LocalSegment>>,
    ),
    String,
> {
    let number_field = |field_name| {
        if let Some(field_str) = captures.name(field_name) {
            match field_str.as_str().parse::<usize>() {
                Ok(number) => Ok(Some(number)),
                // Should be already forbidden by the regex
                Err(err) => Err(format!(
                    "Couldn't parse '{}' as number from {}: {}",
                    field_str.as_str(),
                    field_name,
                    err
                )),
            }
        } else {
            Ok(None)
        }
    };
    let epoch = number_field("epoch")?
        // "If no explicit epoch is given, the implicit epoch is 0"
        .unwrap_or_default();
    let pre = {
        let pre_type = captures
            .name("pre_name")
            .map(|pre| PreRelease::from_str(pre.as_str()))
            // Shouldn't fail due to the regex
            .transpose()?;
        let pre_number = number_field("pre")?
            // <https://peps.python.org/pep-0440/#implicit-pre-release-number>
            .unwrap_or_default();
        pre_type.map(|pre_type| (pre_type, pre_number))
    };
    let post = if captures.name("post_field").is_some() {
        // While PEP 440 says .post is "followed by a non-negative integer value",
        // packaging has tests that ensure that it defaults to 0
        // https://github.com/pypa/packaging/blob/237ff3aa348486cf835a980592af3a59fccd6101/tests/test_version.py#L187-L202
        Some(
            number_field("post_new")?
                .or(number_field("post_old")?)
                .unwrap_or_default(),
        )
    } else {
        None
    };
    let dev = if captures.name("dev_field").is_some() {
        // <https://peps.python.org/pep-0440/#implicit-development-release-number>
        Some(number_field("dev")?.unwrap_or_default())
    } else {
        None
    };
    let local = captures.name("local").map(|local| {
        local
            .as_str()
            .split(&['-', '_', '.'][..])
            .map(|segment| {
                if let Ok(number) = segment.parse::<usize>() {
                    LocalSegment::Number(number)
                } else {
                    // "and if a segment contains any ASCII letters then that segment is compared lexicographically with case insensitivity"
                    LocalSegment::String(segment.to_lowercase())
                }
            })
            .collect()
    });
    Ok((epoch, pre, post, dev, local))
}

impl FromStr for Version {
    type Err = String;

    /// Parses a version such as `1.19`, `1.0a1`,`1.0+abc.5` or `1!2012.2`
    fn from_str(spec: &str) -> Result<Self, Self::Err> {
        let captures = VERSION_RE
            .captures(spec)
            .ok_or_else(|| format!("Version `{}` doesn't match PEP 440 rules", spec))?;
        let (epoch, pre, post, dev, local) = parse_version_modifier(&captures)?;
        let release = captures
            .name("release")
            // Should be forbidden by the regex
            .ok_or_else(|| "No release in version".to_string())?
            .as_str()
            .split('.')
            .map(|segment| {
                if segment == "*" {
                    Err("A star (`*`) must not be used in a fixed version".to_string())
                } else {
                    segment.parse::<usize>().map_err(|err| err.to_string())
                }
            })
            .collect::<Result<Vec<usize>, String>>()?;

        Ok(Version {
            epoch,
            release,
            pre,
            post,
            dev,
            local,
        })
    }
}

impl FromStr for VersionSpecifier {
    type Err = String;

    /// Parses a version such as `>= 1.19`, `== 1.1.*`,`~=1.0+abc.5` or `<=1!2012.2`
    fn from_str(spec: &str) -> Result<Self, Self::Err> {
        let captures = VERSION_SPECIFIER_RE
            .captures(spec)
            .ok_or_else(|| format!("Version specifier `{}` doesn't match PEP 440 rules", spec))?;
        let (epoch, pre, post, dev, local) = parse_version_modifier(&captures)?;

        // operator but we don't know yet if it has a star
        let base_operator = Operator::from_str(&captures["operator"])?;

        let release_str = captures
            .name("release")
            // Should be forbidden by the regex
            .ok_or_else(|| "No release in version".to_string())?
            .as_str()
            .split('.')
            .map(ToString::to_string)
            .collect::<Vec<String>>();

        let mut release_iter = release_str.iter().enumerate().peekable();
        let mut release = Vec::new();

        while let Some((index, entry)) = release_iter.peek().cloned() {
            if *entry == "*" {
                break;
            } else {
                release_iter.next();
            }

            let number = entry
                .parse::<usize>()
                .map_err(|err| format!("Couldn't parse part {} of release: {}", index + 1, err))?;
            release.push(number);
        }

        // Check if there are star versions and if so, switch operator to star version
        let operator = if let Some((first_star_index, _)) = release_iter.peek().cloned() {
            if let Some((non_star_index, _)) = release_iter.find(|(_, entry)| *entry != "*") {
                return Err(format!("A star (`*`) in the version (at position {}) must not be followed by a non-star (at position {})", first_star_index + 1, non_star_index + 1));
            }

            match base_operator {
                Operator::Equal => Operator::EqualStar,
                Operator::NotEqual => Operator::NotEqualStar,
                other => {
                    return Err(format!(
                        "Operator {} must not be used with a star in the version (at position {})",
                        other,
                        first_star_index + 1
                    ))
                }
            }
        } else {
            base_operator
        };

        let version = Version {
            epoch,
            release,
            pre,
            post,
            dev,
            local,
        };

        let version_specifier = VersionSpecifier::new(operator, version)?;
        Ok(version_specifier)
    }
}

/// Parses a list of specifiers such as `>= 1.0, != 1.3.*, < 2.0`
///
/// ```rust
/// use std::str::FromStr;
/// use pep440_rs::{parse_version_specifiers, Version};
///
/// let version = Version::from_str("1.19").unwrap();
/// let version_specifiers = parse_version_specifiers(">=1.16, <2.0").unwrap();
/// assert!(version_specifiers.iter().all(|specifier| specifier.contains(&version)));
/// ```
pub fn parse_version_specifiers(spec: &str) -> Result<Vec<VersionSpecifier>, Pep440Error> {
    let mut version_ranges = Vec::new();
    let mut start: usize = 0;
    let separator = ",";
    for version_range_spec in spec.split(separator) {
        match VersionSpecifier::from_str(version_range_spec) {
            Err(err) => {
                return Err(Pep440Error {
                    message: err,
                    line: spec.to_string(),
                    start,
                    width: version_range_spec.width(),
                });
            }
            Ok(version_range) => {
                version_ranges.push(version_range);
            }
        }
        start += version_range_spec.width();
        start += separator.width();
    }
    Ok(version_ranges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn it_works() {
        let result = parse_version_specifiers("~= 0.9, >= 1.0, != 1.3.4.*, < 2.0").unwrap();
        assert_eq!(
            result,
            [
                VersionSpecifier {
                    operator: Operator::TildeEqual,
                    version: Version {
                        epoch: 0,
                        release: vec![0, 9],
                        pre: None,
                        post: None,
                        dev: None,
                        local: None
                    }
                },
                VersionSpecifier {
                    operator: Operator::GreaterThanEqual,
                    version: Version {
                        epoch: 0,
                        release: vec![1, 0],
                        pre: None,
                        post: None,
                        dev: None,
                        local: None
                    }
                },
                VersionSpecifier {
                    operator: Operator::NotEqualStar,
                    version: Version {
                        epoch: 0,
                        release: vec![1, 3, 4],
                        pre: None,
                        post: None,
                        dev: None,
                        local: None
                    }
                },
                VersionSpecifier {
                    operator: Operator::LessThan,
                    version: Version {
                        epoch: 0,
                        release: vec![2, 0],
                        pre: None,
                        post: None,
                        dev: None,
                        local: None
                    }
                }
            ]
        );
    }

    /// <https://github.com/pypa/packaging/blob/237ff3aa348486cf835a980592af3a59fccd6101/tests/test_version.py#L24-L81>
    #[test]
    fn test_packaging_versions() {
        let versions = [
            // Implicit epoch of 0
            "1.0.dev456",
            "1.0a1",
            "1.0a2.dev456",
            "1.0a12.dev456",
            "1.0a12",
            "1.0b1.dev456",
            "1.0b2",
            "1.0b2.post345.dev456",
            "1.0b2.post345",
            "1.0b2-346",
            "1.0c1.dev456",
            "1.0c1",
            "1.0rc2",
            "1.0c3",
            "1.0",
            "1.0.post456.dev34",
            "1.0.post456",
            "1.1.dev1",
            "1.2+123abc",
            "1.2+123abc456",
            "1.2+abc",
            "1.2+abc123",
            "1.2+abc123def",
            "1.2+1234.abc",
            "1.2+123456",
            "1.2.r32+123456",
            "1.2.rev33+123456",
            // Explicit epoch of 1
            "1!1.0.dev456",
            "1!1.0a1",
            "1!1.0a2.dev456",
            "1!1.0a12.dev456",
            "1!1.0a12",
            "1!1.0b1.dev456",
            "1!1.0b2",
            "1!1.0b2.post345.dev456",
            "1!1.0b2.post345",
            "1!1.0b2-346",
            "1!1.0c1.dev456",
            "1!1.0c1",
            "1!1.0rc2",
            "1!1.0c3",
            "1!1.0",
            "1!1.0.post456.dev34",
            "1!1.0.post456",
            "1!1.1.dev1",
            "1!1.2+123abc",
            "1!1.2+123abc456",
            "1!1.2+abc",
            "1!1.2+abc123",
            "1!1.2+abc123def",
            "1!1.2+1234.abc",
            "1!1.2+123456",
            "1!1.2.r32+123456",
            "1!1.2.rev33+123456",
        ];
        for version in versions {
            Version::from_str(version).unwrap();
            VersionSpecifier::from_str(&format!("=={}", version)).unwrap();
        }
    }

    /// <https://github.com/pypa/packaging/blob/237ff3aa348486cf835a980592af3a59fccd6101/tests/test_version.py#L91-L100>
    #[test]
    fn test_packaging_failures() {
        let versions = [
            // Non sensical versions should be invalid
            "french toast",
            // Versions with invalid local versions
            "1.0+a+",
            "1.0++",
            "1.0+_foobar",
            "1.0+foo&asd",
            "1.0+1+1",
        ];
        for version in versions {
            assert_eq!(
                Version::from_str(version).unwrap_err(),
                format!("Version `{}` doesn't match PEP 440 rules", version)
            );
            assert_eq!(
                VersionSpecifier::from_str(&format!("=={}", version)).unwrap_err(),
                format!(
                    "Version specifier `=={}` doesn't match PEP 440 rules",
                    version
                )
            );
        }
    }

    #[test]
    fn test_equality_and_normalization() {
        let versions = [
            // Various development release incarnations
            ("1.0dev", "1.0.dev0"),
            ("1.0.dev", "1.0.dev0"),
            ("1.0dev1", "1.0.dev1"),
            ("1.0dev", "1.0.dev0"),
            ("1.0-dev", "1.0.dev0"),
            ("1.0-dev1", "1.0.dev1"),
            ("1.0DEV", "1.0.dev0"),
            ("1.0.DEV", "1.0.dev0"),
            ("1.0DEV1", "1.0.dev1"),
            ("1.0DEV", "1.0.dev0"),
            ("1.0.DEV1", "1.0.dev1"),
            ("1.0-DEV", "1.0.dev0"),
            ("1.0-DEV1", "1.0.dev1"),
            // Various alpha incarnations
            ("1.0a", "1.0a0"),
            ("1.0.a", "1.0a0"),
            ("1.0.a1", "1.0a1"),
            ("1.0-a", "1.0a0"),
            ("1.0-a1", "1.0a1"),
            ("1.0alpha", "1.0a0"),
            ("1.0.alpha", "1.0a0"),
            ("1.0.alpha1", "1.0a1"),
            ("1.0-alpha", "1.0a0"),
            ("1.0-alpha1", "1.0a1"),
            ("1.0A", "1.0a0"),
            ("1.0.A", "1.0a0"),
            ("1.0.A1", "1.0a1"),
            ("1.0-A", "1.0a0"),
            ("1.0-A1", "1.0a1"),
            ("1.0ALPHA", "1.0a0"),
            ("1.0.ALPHA", "1.0a0"),
            ("1.0.ALPHA1", "1.0a1"),
            ("1.0-ALPHA", "1.0a0"),
            ("1.0-ALPHA1", "1.0a1"),
            // Various beta incarnations
            ("1.0b", "1.0b0"),
            ("1.0.b", "1.0b0"),
            ("1.0.b1", "1.0b1"),
            ("1.0-b", "1.0b0"),
            ("1.0-b1", "1.0b1"),
            ("1.0beta", "1.0b0"),
            ("1.0.beta", "1.0b0"),
            ("1.0.beta1", "1.0b1"),
            ("1.0-beta", "1.0b0"),
            ("1.0-beta1", "1.0b1"),
            ("1.0B", "1.0b0"),
            ("1.0.B", "1.0b0"),
            ("1.0.B1", "1.0b1"),
            ("1.0-B", "1.0b0"),
            ("1.0-B1", "1.0b1"),
            ("1.0BETA", "1.0b0"),
            ("1.0.BETA", "1.0b0"),
            ("1.0.BETA1", "1.0b1"),
            ("1.0-BETA", "1.0b0"),
            ("1.0-BETA1", "1.0b1"),
            // Various release candidate incarnations
            ("1.0c", "1.0rc0"),
            ("1.0.c", "1.0rc0"),
            ("1.0.c1", "1.0rc1"),
            ("1.0-c", "1.0rc0"),
            ("1.0-c1", "1.0rc1"),
            ("1.0rc", "1.0rc0"),
            ("1.0.rc", "1.0rc0"),
            ("1.0.rc1", "1.0rc1"),
            ("1.0-rc", "1.0rc0"),
            ("1.0-rc1", "1.0rc1"),
            ("1.0C", "1.0rc0"),
            ("1.0.C", "1.0rc0"),
            ("1.0.C1", "1.0rc1"),
            ("1.0-C", "1.0rc0"),
            ("1.0-C1", "1.0rc1"),
            ("1.0RC", "1.0rc0"),
            ("1.0.RC", "1.0rc0"),
            ("1.0.RC1", "1.0rc1"),
            ("1.0-RC", "1.0rc0"),
            ("1.0-RC1", "1.0rc1"),
            // Various post release incarnations
            ("1.0post", "1.0.post0"),
            ("1.0.post", "1.0.post0"),
            ("1.0post1", "1.0.post1"),
            ("1.0post", "1.0.post0"),
            ("1.0-post", "1.0.post0"),
            ("1.0-post1", "1.0.post1"),
            ("1.0POST", "1.0.post0"),
            ("1.0.POST", "1.0.post0"),
            ("1.0POST1", "1.0.post1"),
            ("1.0POST", "1.0.post0"),
            ("1.0r", "1.0.post0"),
            ("1.0rev", "1.0.post0"),
            ("1.0.POST1", "1.0.post1"),
            ("1.0.r1", "1.0.post1"),
            ("1.0.rev1", "1.0.post1"),
            ("1.0-POST", "1.0.post0"),
            ("1.0-POST1", "1.0.post1"),
            ("1.0-5", "1.0.post5"),
            ("1.0-r5", "1.0.post5"),
            ("1.0-rev5", "1.0.post5"),
            // Local version case insensitivity
            ("1.0+AbC", "1.0+abc"),
            // Integer Normalization
            ("1.01", "1.1"),
            ("1.0a05", "1.0a5"),
            ("1.0b07", "1.0b7"),
            ("1.0c056", "1.0rc56"),
            ("1.0rc09", "1.0rc9"),
            ("1.0.post000", "1.0.post0"),
            ("1.1.dev09000", "1.1.dev9000"),
            ("00!1.2", "1.2"),
            ("0100!0.0", "100!0.0"),
            // Various other normalizations
            ("v1.0", "1.0"),
            ("   v1.0\t\n", "1.0"),
        ];
        for (version_str, normalized_str) in versions {
            let version = Version::from_str(version_str).unwrap();
            let normalized = Version::from_str(normalized_str).unwrap();
            // Just test version parsing again
            assert_eq!(version, normalized, "{} {}", version_str, normalized_str);
            // Test version normalization
            assert_eq!(
                version.to_string(),
                normalized.to_string(),
                "{} {}",
                version_str,
                normalized_str
            );
        }
    }

    /// https://github.com/pypa/packaging/blob/237ff3aa348486cf835a980592af3a59fccd6101/tests/test_version.py#L229-L277
    #[test]
    fn test_equality_and_normalization2() {
        let versions = [
            ("1.0.dev456", "1.0.dev456"),
            ("1.0a1", "1.0a1"),
            ("1.0a2.dev456", "1.0a2.dev456"),
            ("1.0a12.dev456", "1.0a12.dev456"),
            ("1.0a12", "1.0a12"),
            ("1.0b1.dev456", "1.0b1.dev456"),
            ("1.0b2", "1.0b2"),
            ("1.0b2.post345.dev456", "1.0b2.post345.dev456"),
            ("1.0b2.post345", "1.0b2.post345"),
            ("1.0rc1.dev456", "1.0rc1.dev456"),
            ("1.0rc1", "1.0rc1"),
            ("1.0", "1.0"),
            ("1.0.post456.dev34", "1.0.post456.dev34"),
            ("1.0.post456", "1.0.post456"),
            ("1.0.1", "1.0.1"),
            ("0!1.0.2", "1.0.2"),
            ("1.0.3+7", "1.0.3+7"),
            ("0!1.0.4+8.0", "1.0.4+8.0"),
            ("1.0.5+9.5", "1.0.5+9.5"),
            ("1.2+1234.abc", "1.2+1234.abc"),
            ("1.2+123456", "1.2+123456"),
            ("1.2+123abc", "1.2+123abc"),
            ("1.2+123abc456", "1.2+123abc456"),
            ("1.2+abc", "1.2+abc"),
            ("1.2+abc123", "1.2+abc123"),
            ("1.2+abc123def", "1.2+abc123def"),
            ("1.1.dev1", "1.1.dev1"),
            ("7!1.0.dev456", "7!1.0.dev456"),
            ("7!1.0a1", "7!1.0a1"),
            ("7!1.0a2.dev456", "7!1.0a2.dev456"),
            ("7!1.0a12.dev456", "7!1.0a12.dev456"),
            ("7!1.0a12", "7!1.0a12"),
            ("7!1.0b1.dev456", "7!1.0b1.dev456"),
            ("7!1.0b2", "7!1.0b2"),
            ("7!1.0b2.post345.dev456", "7!1.0b2.post345.dev456"),
            ("7!1.0b2.post345", "7!1.0b2.post345"),
            ("7!1.0rc1.dev456", "7!1.0rc1.dev456"),
            ("7!1.0rc1", "7!1.0rc1"),
            ("7!1.0", "7!1.0"),
            ("7!1.0.post456.dev34", "7!1.0.post456.dev34"),
            ("7!1.0.post456", "7!1.0.post456"),
            ("7!1.0.1", "7!1.0.1"),
            ("7!1.0.2", "7!1.0.2"),
            ("7!1.0.3+7", "7!1.0.3+7"),
            ("7!1.0.4+8.0", "7!1.0.4+8.0"),
            ("7!1.0.5+9.5", "7!1.0.5+9.5"),
            ("7!1.1.dev1", "7!1.1.dev1"),
        ];
        for (version_str, normalized_str) in versions {
            let version = Version::from_str(version_str).unwrap();
            let normalized = Version::from_str(normalized_str).unwrap();
            assert_eq!(version, normalized, "{} {}", version_str, normalized_str);
            // Test version normalization
            assert_eq!(
                version.to_string(),
                normalized_str,
                "{} {}",
                version_str,
                normalized_str
            );
            // Since we're already at it
            assert_eq!(
                version.to_string(),
                normalized.to_string(),
                "{} {}",
                version_str,
                normalized_str
            );
        }
    }
    #[test]
    fn test_parse_error() {
        let result = parse_version_specifiers("~= 0.9, %‍= 1.0, != 1.3.4.*");
        assert_eq!(
            result.unwrap_err().to_string(),
            indoc! {r#"
                Failed to parse version:
                ~= 0.9, %‍= 1.0, != 1.3.4.*
                       ^^^^^^^
            "#}
        );
    }

    #[test]
    fn test_non_star_after_star() {
        let result = parse_version_specifiers("== 0.9.*.1");
        assert_eq!(
            result.unwrap_err().message,
            "A star (`*`) in the version (at position 3) must not be followed by a non-star (at position 4)"
        );
    }

    #[test]
    fn test_star_wrong_operator() {
        let result = parse_version_specifiers(">= 0.9.1.*");
        assert_eq!(
            result.unwrap_err().message,
            "Operator >= must not be used with a star in the version (at position 4)"
        );
    }

    #[test]
    fn test_star_fixed_version() {
        let result = Version::from_str("0.9.1.*");
        assert_eq!(
            result.unwrap_err(),
            "A star (`*`) must not be used in a fixed version"
        );
    }

    #[test]
    fn test_regex_mismatch() {
        let result = parse_version_specifiers("blergh");
        assert_eq!(
            result.unwrap_err().message,
            "Version specifier `blergh` doesn't match PEP 440 rules"
        );
        let result = Version::from_str("blergh");
        assert_eq!(
            result.unwrap_err(),
            "Version `blergh` doesn't match PEP 440 rules"
        );
    }

    /// <https://github.com/pypa/packaging/blob/e184feef1a28a5c574ec41f5c263a3a573861f5a/tests/test_specifiers.py#L44-L84>
    #[test]
    fn test_invalid_specifier() {
        let specifiers = [
            // Operator-less specifier
            ("2.0", None),
            // Invalid operator
            ("=>2.0", None),
            // Version-less specifier
            ("==", None),
            // Local segment on operators which don't support them
            (
                "~=1.0+5",
                Some("You can't mix a < operator with a local version (`+5`)"),
            ),
            (
                ">=1.0+deadbeef",
                Some("You can't mix a >= operator with a local version (`+deadbeef`)"),
            ),
            (
                "<=1.0+abc123",
                Some("You can't mix a > operator with a local version (`+abc123`)"),
            ),
            (
                ">1.0+watwat",
                Some("You can't mix a >= operator with a local version (`+watwat`)"),
            ),
            (
                "<1.0+1.0",
                Some("You can't mix a <= operator with a local version (`+1.0`)"),
            ),
            // Prefix matching on operators which don't support them
            (
                "~=1.0.*",
                Some("Operator < must not be used with a star in the version (at position 3)"),
            ),
            (
                ">=1.0.*",
                Some("Operator >= must not be used with a star in the version (at position 3)"),
            ),
            (
                "<=1.0.*",
                Some("Operator > must not be used with a star in the version (at position 3)"),
            ),
            (
                ">1.0.*",
                Some("Operator >= must not be used with a star in the version (at position 3)"),
            ),
            (
                "<1.0.*",
                Some("Operator <= must not be used with a star in the version (at position 3)"),
            ),
            // Combination of local and prefix matching on operators which do
            // support one or the other
            (
                "==1.0.*+5",
                Some("You can't mix a == operator with a local version (`+5`)"),
            ),
            (
                "!=1.0.*+deadbeef",
                Some("You can't mix a ~= operator with a local version (`+deadbeef`)"),
            ),
            // Prefix matching cannot be used with a pre-release, post-release,
            // dev or local version
            ("==2.0a1.*", None),
            ("!=2.0a1.*", None),
            ("==2.0.post1.*", None),
            ("!=2.0.post1.*", None),
            ("==2.0.dev1.*", None),
            ("!=2.0.dev1.*", None),
            ("==1.0+5.*", None),
            ("!=1.0+deadbeef.*", None),
            // Prefix matching must appear at the end
            ("==1.0.*.5", Some("A star (`*`) in the version (at position 3) must not be followed by a non-star (at position 4)")),
            // Compatible operator requires 2 digits in the release operator
            ("~=1", Some("The ~= operator requires at least two parts in the release version")),
            // Cannot use a prefix matching after a .devN version
            ("==1.0.dev1.*", None),
            ("!=1.0.dev1.*", None),
        ];
        for (specifier, error) in specifiers {
            if let Some(error) = error {
                assert_eq!(VersionSpecifier::from_str(specifier).unwrap_err(), error)
            } else {
                assert_eq!(
                    VersionSpecifier::from_str(specifier).unwrap_err(),
                    format!(
                        "Version specifier `{}` doesn't match PEP 440 rules",
                        specifier
                    )
                )
            }
        }
    }
}
