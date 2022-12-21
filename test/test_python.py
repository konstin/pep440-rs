"""
This is implementation has some very rudimentary python bindings
"""
from pep440_rs import Version, VersionSpecifier


def test_pep440():
    assert Version("1.1a1").any_prerelease()
    assert Version("1.1.dev2").any_prerelease()
    assert not Version("1.1").any_prerelease()
    assert VersionSpecifier(">=1.0").contains(Version("1.1a1"))
    assert not VersionSpecifier(">=1.1").contains(Version("1.1a1"))
    assert Version("1.1") >= Version("1.1a1")
    assert Version("2.0") in VersionSpecifier("==2")

def test_normalization():
    assert str(Version("1.19-alpha.1")) == "1.19a1"
    assert str(VersionSpecifier(" >=1.19-alpha.1 ")) == ">= 1.19a1"
    assert repr(Version("1.19-alpha.1")) == "1.19a1"
    assert repr(VersionSpecifier(" >=1.19-alpha.1 ")) == ">= 1.19a1"