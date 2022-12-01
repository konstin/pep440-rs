# Reimplementing PEP 440

I've reimplemented [PEP 440](https://peps.python.org/pep-0440/), the python version standard, for [monotrail](https://github.com/konstin/poc-monotrail): [pep440-rs](https://github.com/konstin/pep440-rs). Did you now that `1a1.dev3.post1+deadbeef` is a valid python version, there's not only `==` but also `===` and that version specifiers are context sensitive?

Let's start with the normal stuff: There are basic version numbers with dots in between (like `2.3.1`) and optionally alpha/beta/release candidate suffixes (canonically `2.3.1b1`, but conveniently lenient so `2.3.1-beta.1` also works). For dependencies, there operators for minimums and maximums separated by comma such as `>=2.5.1,<3`. You can of course also select a specific prerelease (e.g. `1.1a1` being matched by `==1.1a1`) and maybe you've also seen constraints like `1.2.*`. But below the clear semver-y surface lie many demons of the old.

![A gloomy forest, one where the demons would hide](https://i.imgur.com/xeN5jZ1.jpeg)

_Photography by [Norbert Buduczki](https://unsplash.com/photos/4IqzgSMrgMk)_

It all starts with the part of the version that's hidden in the default: The epoch. By default it's zero, but if you want to switch versioning system, you can add the new epoch with an exclamation mark like `1!4.2.0`. Since version ordering is defined as a total order, `2020.1` < `1!0.1.0`, but also for some reason `<1!0.1.0` matches `2020.1` and `>2020.1` match `1!0.1.0` (the specifiers are not a total order normally, I don't know why it doesn't specify to never match across epochs). Being a mere mortal, I have never witnessed the turning of epoch myself, but the feature remains part of The Old Code.

You can also add `.dev` and `.post` with some number to all versions, e.g. `1.0.0.dev1` or `1.0.0.post1`. Or just combine them and do `1.0.0.post1.dev1`, which is a developmental release of a post-release. That of course doesn't stop at final releases, you can now do `1.0.0a1.post1.dev1` to have a developmental release of a post release of a prerelease (in canonical form alpha/beta/rc don't have a dot, but dev and post do, while also in PEP 440 dev release are sometime included with the prereleases). If you sort them, obviously `1.0.0.dev1` < `1.0.0` < `1.0.0.post1`, and `1.0.0a1.dev1` < `1.0.0a1` < `1.0.0a1.post1` < `1.0.0`. But dev releases of the final version are sorted lower than any prerelease version, so suddenly we have `1.0.0.dev1`< `1.0.0a1.dev1` < `1.0.0a1` < `1.0.0`, while also having `1.0b2` < `1.0b2.post345.dev456`. That is, the try-out release for 1.0 proper is considered older than the try-out release for the 1.0 alpha. The only sensible way to implement this sorting is making a five-tuple where you map (pre-releasity, pre-number, post-number or None as smallest, dev-number or int max as largest, local version) and let tuple-sorting sort out the rest. Even [pypa/packaging uses tuple logic feat. ±infinity](https://github.com/pypa/packaging/blob/e404434105723a184967b080fc31c05ba69406c6/packaging/version.py#L503-L563).

Matching version with specifiers such as `>=1.2.0` or `<2.0.0` is tricky because PEP 440 says "Pre-releases of any kind, including developmental releases, are implicitly excluded from all version specifiers, unless they are already present on the system, explicitly requested by the user, or if the only available version that satisfies the version specifier is a pre-release". That's really fuzzy, and also it means whether a single version matches a specifier depends on the environment, something I [confused myself with](https://github.com/pypa/packaging/issues/617). pypa effectively says that you must add to specifier whether you want to match prereleases (which here again include dev releases) or not when using the library. The consequence is that when you say `~=2.2` but there's only a `2.2.1a1` it will pick that alpha version (but not `2.2a1`, which never matches).

There are also local versions which can added with a `+` after the regular version, such as `3.4.0+my.local.123.version`. The `123` is going to get ordered as a number, everything that can't be parsed as a number will get ordered as a string. Information about usage is sparse, apparently linux distributions use it to tag their python packaging. Those are also [in semver](https://semver.org/#spec-item-10), but more reasonable as "build metadata": "Build metadata MUST be ignored when determining version precedence. Thus two versions that differ only in the build metadata, have the same precedence".

Finally, there's also `===`, "Arbitrary equality", which is advertised as "simple string equality operations" that "do not take into account any of the semantic information". pypa/packaging has a test that `===lolwat` parses with the comment "=== is an escape hatch in PEP 440". <span style="position:absolute;height:1px;width:1px;overflow:hidden;clip:rect(1px, 1px, 1px, 1px);white-space: nowrap">lolwat I'm freeeeeeeee from PEP 440</span><span aria-hidden="true"><p><span style="display:inline-block;transform:rotateZ(330deg);animation:1s spin infinite ease-in-out 0s alternate"><span style="display:inline-block;transform:rotateZ(-345deg)">lolwat</span></span> I'm fr<span style="display:inline-block;animation:0.5s spin ease-in-out infinite -1.44s alternate;transform:translate(-20%,-20%)">e</span><span style="display:inline-block;animation:0.5s spin ease-in-out infinite -1.28s alternate;transform:translate(-20%,-20%)">e</span><span style="display:inline-block;animation:0.5s spin ease-in-out infinite -1.12s alternate;transform:translate(-20%,-20%)">e</span><span style="display:inline-block;animation:0.5s spin ease-in-out infinite -0.96s alternate;transform:translate(-20%,-20%)">e</span><span style="display:inline-block;animation:0.5s spin ease-in-out infinite -0.8s alternate;transform:translate(-20%,-20%)">e</span><span style="display:inline-block;animation:0.5s spin ease-in-out infinite -0.64s alternate;transform:translate(-20%,-20%)">e</span><span style="display:inline-block;animation:0.5s spin ease-in-out infinite -0.48s alternate;transform:translate(-20%,-20%)">e</span><span style="display:inline-block;animation:0.5s spin ease-in-out infinite -0.32s alternate;transform:translate(-20%,-20%)">e</span><span style="display:inline-block;animation:0.5s spin ease-in-out infinite -0.16s alternate;transform:translate(-20%,-20%)">e</span> from PEP 440</p></span>

---

For those wondering why python[^1] didn't pick a sane standard like semver to begin with, the basic syntax format for writing things like `>1.0, !=1.3.4, <2.0` was written down in [PEP 314](https://peps.python.org/pep-0314/#requires-multiple-use) in 2003 (!) [^2]. [PEP 386](https://peps.python.org/pep-0386/), the first python version standard, was written in 2009, "codifying existing practices" and its successor and current standard [PEP 440](https://peps.python.org/pep-0440/) in 2013. In comparison, the first commit to npm was made in 2010, semver v1.0.0 was published in 2011, v2.0.0 in 2013, npm inc. was founded in 2014 and cargo had its first commit also in 2014. So python has a hard time doing modern packaging because they were trying to do modern packaging before it was being invented.

I still believe that (a) bringing in features from semver and tools such as poetry, cargo and npm would greatly benefit the python ecosystem and (b) python packaging isn't doomed to stay in its current state. While e.g. pypi's backend will have to handle everything that ever used to be legal, i believe that the ecosystem at large can and must migrate to better tools and standards. This is largely informed by having to deal with a lot of the breakages of the current state of python packaging and trying to support friends and colleagues.

The easiest is probably to deprecate `===`, even PEP 440 soft-deprecates it with "Use of this operator is heavily discouraged and tooling MAY display a warning when it is used".

For epochs, i haven't seen them used even a single time. To try to make this at least a bit empirical i ran two queries on the pypi bigquery data[^3][^4], with the result that in one month there were 19,699,031,713 downloads, 40,281 of which for versions specifying an epoch, that's 0.0002%.

Post releases can be replaced with publishing a new patch release or a one-higher pre-release. Historically, it was a good idea that you could specify `1.2.3` and if the author messed up `1.2.3` and has to publish fixup wheels you'd be directly moved to the fixup, but nowadays you want lock files where this doesn't work anymore, and it also interacts weirdly with yanking. This applies especially to post releases of prereleases, which PEP 440 acknowledges: "Creating post-releases of pre-releases is strongly discouraged, as it makes the version identifier difficult to parse for human readers. In general, it is substantially clearer to simply create a new pre-release by incrementing the numeric component".

Dev version on prereleases seem also strange to me (just publish a higher prerelease instead, a test-release of a test-release is kinda redundant). For dev versions of final releases there are certainly workflows that benefit from them[^5], even though other ecosystems do fine without special casing `.dev`. The main problem are the strange semantics, and while PEP 440 defends this as "far more logical sort order", i strongly disagree, this was and is super confusing, and also the implementation is a mess. When removing dev (and ideally also post) releases at least for alpha/beta/rc versions, the semantics would become intuitive again, with dev release simply being a prerelease one level below alpha releases[^6].

For local version the semver style "purely informative and no semantics" definition would be imho more reasonable; i unfortunately can't tell if de-semanticizing local version would break anything (as in, is anybody currently depending on the fact that `1.0+foo.10` has precedence over `1.0+foo.9`).

Given that [pip now has a backtracking dependency resolver](https://pip.pypa.io/en/stable/topics/dependency-resolution/#backtracking), i think we can simplify the spec a lot by separating it into three parts: One part that defines the version number schema and precedence (a total order as it currently is), one part that translates operators such as `~=` into normal `>`/`=`/`>` sets that directly translate to the version order, and one part that specifies the rules for resolvers, that is when are they allowed to pick which prerelease. The latter isn't well-defined as of PEP 440, but imho we should agree about this across the ecosystem[^7]. See e.g. [node on prereleases](https://github.com/npm/node-semver#prerelease-tags) and [cargo on prereleases](https://doc.rust-lang.org/cargo/reference/resolver.html#pre-releases)[^8]. I particularly like the node/npm "If a version has a prerelease tag (for example, `1.2.3-alpha.3`) then it will only be allowed to satisfy comparator sets if at least one comparator with the same `[major, minor, patch]` tuple also has a prerelease tag". For comparison, firefox estimates 16–20 minutes for the semver spec, but 57–73 minutes for PEP 440.

For all change there would need to be long announcement and deprecation periods with a specific focus on helping people migrate their workflows. For the deprecation period, tools should print big red warnings whenever they encounter something broken. Speaking of announcements, there's really a lack of an official pypa communication channel! An official blog for announcements on deprecations, changes, release, and (proposed) PEP status changes together with a community aggregator like [This Week in Rust](https://this-week-in-rust.org) would be extremely helpful over the current word-of-mouth-in-twitter-replies-and-buried-github-issues system.

Two features that would be great to add are the caret operator (`^`) and the tilde operator (`~`) from semver. Nowadays semver is arguably the most popular version scheme even in python, and for most packages you want `^1.2.3` and for the remainder (including calver projects that treat the last digit as semver-like patch version) `~1.8` will do the right thing. I'd like to add them to pep440-rs eventually but i'm neither sure about the exact semantic yet nor how to let users switch between PEP 440-only specifiers and the modern superset. 

Next Up: PEP 508 

[^1]: Well, technically not python as the python interpreter but pypa as the vague group of people who make the packaging PEPs. Python itself didn't even have a concept of package versions at all until `importlib.metadata` introduced optionally reading a version as a string to the standard library, and the language itself still doesn't have a concept of packages but merely one of modules. When you `import foo` it effectively just asks `sys.meta_path` if anyone can import foo, which will check if any location in `sys.path` has a `foo` module, but this has no relation to packaging. If you ask stdlib's `importlib.metadata` for an installed package version, it [really just asks `sys.meta_path` with a different method if anyone optionally wants to tell it about the package version](https://github.com/python/cpython/blob/8af04cdef202364541540ed67e204b71e2e759d0/Lib/importlib/metadata/__init__.py#L362-L413), which by default will just look for `.dist-info` folders in your `sys.path`.
[^2]: If you ever wondered why wheel metadata is in some archaic e-mail-headers RFC 822 

  <div style="text-align: center">

  ```
  STANDARD FOR THE FORMAT OF
  
  ARPA INTERNET TEXT MESSAGES
  ```

  </div>

  that's because it was [picked in 2001](https://peps.python.org/pep-0241/). Even XML 1.0 was [published just 3 years prior](https://www.w3.org/TR/1998/REC-xml-19980210.html). I'm still very much in favor of [migrating to a JSON or TOML format](https://peps.python.org/pep-0566/#json-compatible-metadata) such as `pkg-info.json` or editing `pyproject.toml` similar to what cargo does, but that's for another time.
[^3]: I ran this on 2022-11-29 and the queries were
    ```sql
    SELECT
      COUNT(*)
    FROM
      bigquery-public-data.pypi.file_downloads
    WHERE
      timestamp BETWEEN TIMESTAMP(DATETIME_SUB(CURRENT_DATETIME(), INTERVAL 1 MONTH))
      AND TIMESTAMP(CURRENT_DATETIME())
    ```
    and
    ```sql
    SELECT
      COUNT(*)
    FROM
      bigquery-public-data.pypi.file_downloads
    WHERE
      timestamp BETWEEN TIMESTAMP(DATETIME_SUB(CURRENT_DATETIME(), INTERVAL 1 MONTH))
      AND TIMESTAMP(CURRENT_DATETIME())
      AND CONTAINS_SUBSTR(file.version, '!')
    ```
[^4]: Blessed be whoever came up with the [bigquery datasets for pypi](https://warehouse.pypa.io/api-reference/bigquery-datasets.html)
[^5]: E.g. some people want to build `{Major}.{Minor}.{Patch}.dev{YYYY}{MM}{DD}{MonotonicallyIncreasingDailyBuildNumber}` in their CI workflows. Local versions are used to indicate when linux distributions did some downstream packing, so you can directly tell when you're looking at a distro patched install.
[^6]: I'm still not sure if they provide any benefit over just using alpha versions, but once they behave like normal prereleases their implementation and cognitive overhead is near zero so backwards compatibility is way more significant. Note that semver relies on "alpha", "beta" and "rc" being alphabetically ordered, while we need to make "dev" lowest manually, otoh semver also allows any random stuff for prereleases and uses the same duck-typed logic for comparing them as PEP 440 uses for local versions.
[^7]: Consider the case where a user adds a library `A` from pypi that has multiple transitive dependencies on `B`, some specifier with preleases in their specifiers and some without. It would be bad for the authors of `A` to work if they couldn't clearly reason which prereleases of `B` might or might not be picked independent of which tool the user uses.
[^8]: According to the python survey results, those are the two most popular other package managers in use
    
  ![Plot showing bars on how much other package managers are being used, with docker, npm, cargo and yarn on top](https://i.imgur.com/b1Jolbk.png)

  RubyGems [states](https://guides.rubygems.org/patterns/) "The RubyGems team urges gem developers to follow the Semantic Versioning standard for their gem’s versions. The RubyGems library itself does not enforce a strict versioning policy, but using an “irrational” policy will only be a disservice to those in the community who use your gems", but i couldn't find any details on what versions and operators are allowed.

  Composer on the other hand is very much like python ([docs](https://getcomposer.org/doc/04-schema.md#version)): "This must follow the format of X.Y.Z or vX.Y.Z with an optional suffix of -dev, -patch (-p), -alpha (-a), -beta (-b) or -RC.", where dev is below alpha. It also seems to allow `1.2.*` but i couldn't find any more documentation on what's allowed and what the semantics are except that they apparently [transform prereleases to a version digit](https://github.com/composer/composer/blob/bd6a5019b3bf5edf13640522796f54accaad789e/src/Composer/Platform/Version.php#L63-L69) 
