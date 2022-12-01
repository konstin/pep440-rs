use crate::types::VersionSpecifier;
use crate::{LocalSegment, Operator, PreRelease, Version};
#[cfg(feature = "pyo3")]
use pyo3::pymethods;
use std::cmp::Ordering;
use std::iter;
use tracing::warn;

/// Compare the release parts of two versions, e.g. `4.3.1` > `4.2`, `1.1.0` == `1.1` and
/// `1.16` < `1.19`
fn compare_release(this: &[usize], other: &[usize]) -> Ordering {
    // "When comparing release segments with different numbers of components, the shorter segment
    // is padded out with additional zeros as necessary"
    let iterator: Vec<(&usize, &usize)> = if this.len() < other.len() {
        this.iter().chain(iter::repeat(&0)).zip(other).collect()
    } else {
        this.iter()
            .zip(other.iter().chain(iter::repeat(&0)))
            .collect()
    };

    for (a, b) in iterator {
        if a != b {
            return a.cmp(b);
        }
    }

    Ordering::Equal
}

/// Compare the parts attached after the release, given equal release
///
/// According to <https://peps.python.org/pep-0440/#summary-of-permitted-suffixes-and-relative-ordering>
/// the order of pre/post-releases is:
/// .devN, aN, bN, rcN, <no suffix (final)>, .postN
/// but also, you can have dev/post releases on beta releases, so we make a three stage ordering:
/// ({dev: 0, a: 1, b: 2, rc: 3, (): 4, post: 5}, <preN>, <postN or None as smallest>, <devN or Max as largest>, <local>)
///
/// For post, any number is better than none (so None defaults to None<0), but for dev, no number
/// is better (so None default to the maximum). For local the Option<Vec<T>> luckily already has the
/// correct default Ord implementation
fn sortable_tuple(
    version: &Version,
) -> (
    usize,
    usize,
    Option<usize>,
    usize,
    Option<Vec<LocalSegment>>,
) {
    match (&version.pre, &version.post, &version.dev) {
        // dev release
        (None, None, Some(n)) => (0, 0, None, *n, version.local.clone()),
        // alpha release
        (Some((PreRelease::Alpha, n)), post, dev) => (
            1,
            *n,
            *post,
            dev.unwrap_or(usize::MAX),
            version.local.clone(),
        ),
        // beta release
        (Some((PreRelease::Beta, n)), post, dev) => (
            2,
            *n,
            *post,
            dev.unwrap_or(usize::MAX),
            version.local.clone(),
        ),
        // alpha release
        (Some((PreRelease::Rc, n)), post, dev) => (
            3,
            *n,
            *post,
            dev.unwrap_or(usize::MAX),
            version.local.clone(),
        ),
        // final release
        (None, None, None) => (4, 0, None, 0, version.local.clone()),
        // post release
        (None, Some(post), dev) => (
            5,
            0,
            Some(*post),
            dev.unwrap_or(usize::MAX),
            version.local.clone(),
        ),
    }
}

impl PartialEq<Self> for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl PartialOrd<Self> for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    /// 1.0.dev456 < 1.0a1 < 1.0a2.dev456 < 1.0a12.dev456 < 1.0a12 < 1.0b1.dev456 < 1.0b2
    /// < 1.0b2.post345.dev456 < 1.0b2.post345 < 1.0b2-346 < 1.0c1.dev456 < 1.0c1 < 1.0rc2 < 1.0c3
    /// < 1.0 < 1.0.post456.dev34 < 1.0.post456
    fn cmp(&self, other: &Self) -> Ordering {
        if self.epoch != other.epoch {
            return self.epoch.cmp(&other.epoch);
        }

        match compare_release(&self.release, &other.release) {
            Ordering::Less => {
                return Ordering::Less;
            }
            Ordering::Equal => {}
            Ordering::Greater => {
                return Ordering::Greater;
            }
        }
        // release is equal, so compare the other parts
        sortable_tuple(self).cmp(&sortable_tuple(other))
    }
}

impl Ord for LocalSegment {
    fn cmp(&self, other: &Self) -> Ordering {
        // <https://peps.python.org/pep-0440/#local-version-identifiers>
        match (self, other) {
            (Self::Number(n1), Self::Number(n2)) => n1.cmp(n2),
            (Self::String(s1), Self::String(s2)) => s1.cmp(s2),
            (Self::Number(_), Self::String(_)) => Ordering::Greater,
            (Self::String(_), Self::Number(_)) => Ordering::Less,
        }
    }
}

#[cfg_attr(feature = "pyo3", pymethods)]
impl VersionSpecifier {
    /// Whether the given version satisfies the version range
    ///
    /// e.g. `>=1.19,<2.0` and `1.21` -> true
    /// <https://peps.python.org/pep-0440/#version-specifiers>
    ///
    /// Unlike `pypa/packaging`, prereleases are included by default
    ///
    /// I'm utterly non-confident in the description in PEP 440 and apparently even pip got some
    /// of that wrong, e.g. <https://github.com/pypa/pip/issues/9121> and
    /// <https://github.com/pypa/pip/issues/5503>, and also i'm not sure if it produces the correct
    /// behaviour from a user perspective
    ///
    /// This implementation is as close as possible to
    /// <https://github.com/pypa/packaging/blob/e184feef1a28a5c574ec41f5c263a3a573861f5a/packaging/specifiers.py#L362-L496>
    pub fn contains(&self, version: &Version) -> bool {
        // "Except where specifically noted below, local version identifiers MUST NOT be permitted
        // in version specifiers, and local version labels MUST be ignored entirely when checking
        // if candidate versions match a given version specifier."
        let (this, other) = if self.version.local.is_some() {
            (self.version.clone(), version.clone())
        } else {
            // self is already without local
            (self.version.without_local(), version.without_local())
        };

        match self.operator {
            Operator::Equal => other == this,
            Operator::EqualStar => {
                this.epoch == other.epoch
                    && self
                        .version
                        .release
                        .iter()
                        .zip(&other.release)
                        .all(|(this, other)| this == other)
            }
            #[allow(deprecated)]
            Operator::ExactEqual => {
                warn!("Using arbitrary equality (`===`) is discouraged");
                self.version.to_string() == version.to_string()
            }
            Operator::NotEqual => other != this,
            Operator::NotEqualStar => {
                this.epoch != other.epoch
                    || !this
                        .release
                        .iter()
                        .zip(&version.release)
                        .all(|(this, other)| this == other)
            }
            Operator::TildeEqual => {
                // "For a given release identifier V.N, the compatible release clause is
                // approximately equivalent to the pair of comparison clauses: `>= V.N, == V.*`"
                // First, we test that every but the last digit matches.
                // We know that this must hold true since we checked it in the constructor
                assert!(this.release.len() > 1);
                if this.epoch != other.epoch {
                    return false;
                }

                if !this.release[..this.release.len() - 1]
                    .iter()
                    .zip(&other.release)
                    .all(|(this, other)| this == other)
                {
                    return false;
                }

                // According to PEP 440, this ignores the prerelease special rules
                // pypa/packaging disagrees: https://github.com/pypa/packaging/issues/617
                other >= this
            }
            Operator::GreaterThan => Self::greater_than(&this, &other),
            Operator::GreaterThanEqual => Self::greater_than(&this, &other) || other >= this,
            Operator::LessThan => {
                Self::less_than(&this, &other)
                    && !(compare_release(&this.release, &other.release) == Ordering::Equal
                        && other.any_prerelease())
            }
            Operator::LessThanEqual => Self::less_than(&this, &other) || other <= this,
        }
    }
}

impl VersionSpecifier {
    fn less_than(this: &Version, other: &Version) -> bool {
        if other.epoch < this.epoch {
            return true;
        }

        // This special case is here so that, unless the specifier itself
        // includes is a pre-release version, that we do not accept pre-release
        // versions for the version mentioned in the specifier (e.g. <3.1 should
        // not match 3.1.dev0, but should match 3.0.dev0).
        if !this.any_prerelease()
            && other.is_pre()
            && compare_release(&this.release, &other.release) == Ordering::Equal
        {
            return false;
        }

        other < this
    }

    fn greater_than(this: &Version, other: &Version) -> bool {
        if other.epoch > this.epoch {
            return true;
        }

        if compare_release(&this.release, &other.release) == Ordering::Equal {
            // This special case is here so that, unless the specifier itself
            // includes is a post-release version, that we do not accept
            // post-release versions for the version mentioned in the specifier
            // (e.g. >3.1 should not match 3.0.post0, but should match 3.2.post0).
            if !this.is_post() && other.is_post() {
                return false;
            }

            // We already checked that self doesn't have a local version
            if other.is_local() {
                return false;
            }
        }

        other > this
    }
}

#[cfg(test)]
mod test {
    use crate::{Version, VersionSpecifier};
    use std::cmp::Ordering;
    use std::str::FromStr;

    /// <https://peps.python.org/pep-0440/#version-matching>
    #[test]
    fn test_equal() {
        let version = Version::from_str("1.1.post1").unwrap();

        assert!(!VersionSpecifier::from_str("== 1.1")
            .unwrap()
            .contains(&version));
        assert!(VersionSpecifier::from_str("== 1.1.post1")
            .unwrap()
            .contains(&version));
        assert!(VersionSpecifier::from_str("== 1.1.*")
            .unwrap()
            .contains(&version));
    }

    const VERSIONS_ALL: &[&str] = &[
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

    /// https://github.com/pypa/packaging/blob/237ff3aa348486cf835a980592af3a59fccd6101/tests/test_version.py#L666-L707
    /// https://github.com/pypa/packaging/blob/237ff3aa348486cf835a980592af3a59fccd6101/tests/test_version.py#L709-L750
    ///
    /// These tests are a lot shorter than the pypa/packaging version since we implement all
    /// comparisons through one method
    #[test]
    fn test_operators_true() {
        let versions: Vec<Version> = VERSIONS_ALL
            .iter()
            .map(|version| Version::from_str(version).unwrap())
            .collect();

        // Below we'll generate every possible combination of VERSIONS_ALL that
        // should be true for the given operator
        let operations: Vec<_> = [
            // Verify that the less than (<) operator works correctly
            versions
                .iter()
                .enumerate()
                .flat_map(|(i, x)| {
                    versions[i + 1..]
                        .iter()
                        .map(move |y| (x, y, Ordering::Less))
                })
                .collect::<Vec<_>>(),
            // Verify that the equal (==) operator works correctly
            versions
                .iter()
                .map(move |x| (x, x, Ordering::Equal))
                .collect::<Vec<_>>(),
            // Verify that the greater than (>) operator works correctly
            versions
                .iter()
                .enumerate()
                .flat_map(|(i, x)| versions[..i].iter().map(move |y| (x, y, Ordering::Greater)))
                .collect::<Vec<_>>(),
        ]
        .into_iter()
        .flatten()
        .collect();

        for (a, b, ordering) in operations {
            assert_eq!(a.cmp(b), ordering, "{} {:?} {}", a, ordering, b);
        }
    }

    const VERSIONS_0: &[&str] = &[
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
    ];

    const SPECIFIERS_OTHER: &[&str] = &[
        "== 1.*", "== 1.0.*", "== 1.1.*", "== 1.2.*", "== 2.*", "~= 1.0", "~= 1.0b1", "~= 1.1",
        "~= 1.2", "~= 2.0",
    ];

    const EXPECTED_OTHER: &[[bool; 10]] = &[
        [
            true, true, false, false, false, false, false, false, false, false,
        ],
        [
            true, true, false, false, false, false, false, false, false, false,
        ],
        [
            true, true, false, false, false, false, false, false, false, false,
        ],
        [
            true, true, false, false, false, false, false, false, false, false,
        ],
        [
            true, true, false, false, false, false, false, false, false, false,
        ],
        [
            true, true, false, false, false, false, false, false, false, false,
        ],
        [
            true, true, false, false, false, false, true, false, false, false,
        ],
        [
            true, true, false, false, false, false, true, false, false, false,
        ],
        [
            true, true, false, false, false, false, true, false, false, false,
        ],
        [
            true, true, false, false, false, false, true, false, false, false,
        ],
        [
            true, true, false, false, false, false, true, false, false, false,
        ],
        [
            true, true, false, false, false, false, true, false, false, false,
        ],
        [
            true, true, false, false, false, false, true, false, false, false,
        ],
        [
            true, true, false, false, false, false, true, false, false, false,
        ],
        [
            true, true, false, false, false, true, true, false, false, false,
        ],
        [
            true, true, false, false, false, true, true, false, false, false,
        ],
        [
            true, true, false, false, false, true, true, false, false, false,
        ],
        [
            true, false, true, false, false, true, true, false, false, false,
        ],
        [
            true, false, false, true, false, true, true, true, true, false,
        ],
        [
            true, false, false, true, false, true, true, true, true, false,
        ],
        [
            true, false, false, true, false, true, true, true, true, false,
        ],
        [
            true, false, false, true, false, true, true, true, true, false,
        ],
        [
            true, false, false, true, false, true, true, true, true, false,
        ],
        [
            true, false, false, true, false, true, true, true, true, false,
        ],
        [
            true, false, false, true, false, true, true, true, true, false,
        ],
        [
            true, false, false, true, false, true, true, true, true, false,
        ],
        [
            true, false, false, true, false, true, true, true, true, false,
        ],
    ];

    /// Test for tilde equal (~=) and star equal (== x.y.*) recorded from pypa/packaging
    ///
    /// Well, except for https://github.com/pypa/packaging/issues/617
    #[test]
    fn test_operators_other() {
        let versions: Vec<Version> = VERSIONS_0
            .iter()
            .map(|version| Version::from_str(version).unwrap())
            .collect();
        let specifiers: Vec<_> = SPECIFIERS_OTHER
            .iter()
            .map(|specifier| VersionSpecifier::from_str(specifier).unwrap())
            .collect();

        for (version, expected) in versions.iter().zip(EXPECTED_OTHER) {
            let actual = specifiers
                .iter()
                .map(|specifier| specifier.contains(version))
                .collect::<Vec<bool>>();
            for ((actual, expected), specifier) in actual.iter().zip(expected).zip(SPECIFIERS_OTHER)
            {
                assert_eq!(actual, expected, "{} {}", version, specifier);
            }
        }
    }

    #[test]
    fn test_arbitrary_equality() {
        assert!(VersionSpecifier::from_str("=== 1.2a1")
            .unwrap()
            .contains(&Version::from_str("1.2a1").unwrap()));
        assert!(!VersionSpecifier::from_str("=== 1.2a1")
            .unwrap()
            .contains(&Version::from_str("1.2a1+local").unwrap()));
    }

    #[test]
    fn test_specifiers_true() {
        let pairs = [
            // Test the equality operation
            ("2.0", "==2"),
            ("2.0", "==2.0"),
            ("2.0", "==2.0.0"),
            ("2.0+deadbeef", "==2"),
            ("2.0+deadbeef", "==2.0"),
            ("2.0+deadbeef", "==2.0.0"),
            ("2.0+deadbeef", "==2+deadbeef"),
            ("2.0+deadbeef", "==2.0+deadbeef"),
            ("2.0+deadbeef", "==2.0.0+deadbeef"),
            ("2.0+deadbeef.0", "==2.0.0+deadbeef.00"),
            // Test the equality operation with a prefix
            ("2.dev1", "==2.*"),
            ("2a1", "==2.*"),
            ("2a1.post1", "==2.*"),
            ("2b1", "==2.*"),
            ("2b1.dev1", "==2.*"),
            ("2c1", "==2.*"),
            ("2c1.post1.dev1", "==2.*"),
            ("2c1.post1.dev1", "==2.0.*"),
            ("2rc1", "==2.*"),
            ("2rc1", "==2.0.*"),
            ("2", "==2.*"),
            ("2", "==2.0.*"),
            ("2", "==0!2.*"),
            ("0!2", "==2.*"),
            ("2.0", "==2.*"),
            ("2.0.0", "==2.*"),
            ("2.1+local.version", "==2.1.*"),
            // Test the in-equality operation
            ("2.1", "!=2"),
            ("2.1", "!=2.0"),
            ("2.0.1", "!=2"),
            ("2.0.1", "!=2.0"),
            ("2.0.1", "!=2.0.0"),
            ("2.0", "!=2.0+deadbeef"),
            // Test the in-equality operation with a prefix
            ("2.0", "!=3.*"),
            ("2.1", "!=2.0.*"),
            // Test the greater than equal operation
            ("2.0", ">=2"),
            ("2.0", ">=2.0"),
            ("2.0", ">=2.0.0"),
            ("2.0.post1", ">=2"),
            ("2.0.post1.dev1", ">=2"),
            ("3", ">=2"),
            // Test the less than equal operation
            ("2.0", "<=2"),
            ("2.0", "<=2.0"),
            ("2.0", "<=2.0.0"),
            ("2.0.dev1", "<=2"),
            ("2.0a1", "<=2"),
            ("2.0a1.dev1", "<=2"),
            ("2.0b1", "<=2"),
            ("2.0b1.post1", "<=2"),
            ("2.0c1", "<=2"),
            ("2.0c1.post1.dev1", "<=2"),
            ("2.0rc1", "<=2"),
            ("1", "<=2"),
            // Test the greater than operation
            ("3", ">2"),
            ("2.1", ">2.0"),
            ("2.0.1", ">2"),
            ("2.1.post1", ">2"),
            ("2.1+local.version", ">2"),
            // Test the less than operation
            ("1", "<2"),
            ("2.0", "<2.1"),
            ("2.0.dev0", "<2.1"),
            // Test the compatibility operation
            ("1", "~=1.0"),
            ("1.0.1", "~=1.0"),
            ("1.1", "~=1.0"),
            ("1.9999999", "~=1.0"),
            ("1.1", "~=1.0a1"),
            ("2022.01.01", "~=2022.01.01"),
            // Test that epochs are handled sanely
            ("2!1.0", "~=2!1.0"),
            ("2!1.0", "==2!1.*"),
            ("2!1.0", "==2!1.0"),
            ("2!1.0", "!=1.0"),
            ("1.0", "!=2!1.0"),
            ("1.0", "<=2!0.1"),
            ("2!1.0", ">=2.0"),
            ("1.0", "<2!0.1"),
            ("2!1.0", ">2.0"),
            // Test some normalization rules
            ("2.0.5", ">2.0dev"),
        ];

        for (version, specifier) in pairs {
            assert!(
                VersionSpecifier::from_str(specifier)
                    .unwrap()
                    .contains(&Version::from_str(version).unwrap()),
                "{} {}",
                version,
                specifier
            );
        }
    }

    #[test]
    fn test_specifier_false() {
        let pairs = [
            // Test the equality operation
            ("2.1", "==2"),
            ("2.1", "==2.0"),
            ("2.1", "==2.0.0"),
            ("2.0", "==2.0+deadbeef"),
            // Test the equality operation with a prefix
            ("2.0", "==3.*"),
            ("2.1", "==2.0.*"),
            // Test the in-equality operation
            ("2.0", "!=2"),
            ("2.0", "!=2.0"),
            ("2.0", "!=2.0.0"),
            ("2.0+deadbeef", "!=2"),
            ("2.0+deadbeef", "!=2.0"),
            ("2.0+deadbeef", "!=2.0.0"),
            ("2.0+deadbeef", "!=2+deadbeef"),
            ("2.0+deadbeef", "!=2.0+deadbeef"),
            ("2.0+deadbeef", "!=2.0.0+deadbeef"),
            ("2.0+deadbeef.0", "!=2.0.0+deadbeef.00"),
            // Test the in-equality operation with a prefix
            ("2.dev1", "!=2.*"),
            ("2a1", "!=2.*"),
            ("2a1.post1", "!=2.*"),
            ("2b1", "!=2.*"),
            ("2b1.dev1", "!=2.*"),
            ("2c1", "!=2.*"),
            ("2c1.post1.dev1", "!=2.*"),
            ("2c1.post1.dev1", "!=2.0.*"),
            ("2rc1", "!=2.*"),
            ("2rc1", "!=2.0.*"),
            ("2", "!=2.*"),
            ("2", "!=2.0.*"),
            ("2.0", "!=2.*"),
            ("2.0.0", "!=2.*"),
            // Test the greater than equal operation
            ("2.0.dev1", ">=2"),
            ("2.0a1", ">=2"),
            ("2.0a1.dev1", ">=2"),
            ("2.0b1", ">=2"),
            ("2.0b1.post1", ">=2"),
            ("2.0c1", ">=2"),
            ("2.0c1.post1.dev1", ">=2"),
            ("2.0rc1", ">=2"),
            ("1", ">=2"),
            // Test the less than equal operation
            ("2.0.post1", "<=2"),
            ("2.0.post1.dev1", "<=2"),
            ("3", "<=2"),
            // Test the greater than operation
            ("1", ">2"),
            ("2.0.dev1", ">2"),
            ("2.0a1", ">2"),
            ("2.0a1.post1", ">2"),
            ("2.0b1", ">2"),
            ("2.0b1.dev1", ">2"),
            ("2.0c1", ">2"),
            ("2.0c1.post1.dev1", ">2"),
            ("2.0rc1", ">2"),
            ("2.0", ">2"),
            ("2.0.post1", ">2"),
            ("2.0.post1.dev1", ">2"),
            ("2.0+local.version", ">2"),
            // Test the less than operation
            ("2.0.dev1", "<2"),
            ("2.0a1", "<2"),
            ("2.0a1.post1", "<2"),
            ("2.0b1", "<2"),
            ("2.0b2.dev1", "<2"),
            ("2.0c1", "<2"),
            ("2.0c1.post1.dev1", "<2"),
            ("2.0rc1", "<2"),
            ("2.0", "<2"),
            ("2.post1", "<2"),
            ("2.post1.dev1", "<2"),
            ("3", "<2"),
            // Test the compatibility operation
            ("2.0", "~=1.0"),
            ("1.1.0", "~=1.0.0"),
            ("1.1.post1", "~=1.0.0"),
            // Test that epochs are handled sanely
            ("1.0", "~=2!1.0"),
            ("2!1.0", "~=1.0"),
            ("2!1.0", "==1.0"),
            ("1.0", "==2!1.0"),
            ("2!1.0", "==1.*"),
            ("1.0", "==2!1.*"),
            ("2!1.0", "!=2!1.0"),
        ];
        for (version, specifier) in pairs {
            assert!(
                !VersionSpecifier::from_str(specifier)
                    .unwrap()
                    .contains(&Version::from_str(version).unwrap()),
                "{} {}",
                version,
                specifier
            );
        }
    }
}
