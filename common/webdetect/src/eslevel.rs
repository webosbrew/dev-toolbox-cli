//! ECMAScript feature/level model and its mapping to Chromium versions.
//!
//! A web app's shipped bundle uses some set of JS syntax features; the *highest*
//! one determines the minimum engine it can run on. We express that as an
//! [`EsLevel`] and map it to the minimum Chromium major that ships the syntax,
//! so a firmware's web-engine version can be turned into a pass/fail verdict.

/// A coarse ECMAScript level, ordered oldest → newest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EsLevel {
    Es5,
    Es2015,
    Es2016,
    Es2017,
    Es2018,
    Es2019,
    Es2020,
    Es2021Plus,
}

impl EsLevel {
    /// Minimum Chromium major that natively supports the syntax this level
    /// implies. Values are the standard caniuse/V8 landing points.
    pub fn min_chromium_major(self) -> u32 {
        match self {
            EsLevel::Es5 => 0, // universally supported
            EsLevel::Es2015 => 49, // let/const, arrow, class, template, spread
            EsLevel::Es2016 => 52, // ** exponentiation
            EsLevel::Es2017 => 55, // async/await
            EsLevel::Es2018 => 60, // object spread, async iteration
            EsLevel::Es2019 => 73,
            EsLevel::Es2020 => 80, // optional chaining, nullish coalescing
            EsLevel::Es2021Plus => 85,
        }
    }

    /// Highest [`EsLevel`] a Chromium engine of the given major supports — the
    /// inverse of [`EsLevel::min_chromium_major`].
    pub fn from_chromium_major(major: u32) -> EsLevel {
        for level in [
            EsLevel::Es2021Plus,
            EsLevel::Es2020,
            EsLevel::Es2019,
            EsLevel::Es2018,
            EsLevel::Es2017,
            EsLevel::Es2016,
            EsLevel::Es2015,
        ] {
            if major >= level.min_chromium_major() {
                return level;
            }
        }
        EsLevel::Es5
    }

    pub fn label(self) -> &'static str {
        match self {
            EsLevel::Es5 => "ES5",
            EsLevel::Es2015 => "ES2015",
            EsLevel::Es2016 => "ES2016",
            EsLevel::Es2017 => "ES2017",
            EsLevel::Es2018 => "ES2018",
            EsLevel::Es2019 => "ES2019",
            EsLevel::Es2020 => "ES2020",
            EsLevel::Es2021Plus => "ES2021+",
        }
    }
}

/// A concrete JS syntax feature detected in a bundle, used as evidence for the
/// derived [`EsLevel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EsFeature {
    LetConst,
    Arrow,
    TemplateLiteral,
    Class,
    Spread,
    Exponent,
    AsyncAwait,
    OptionalChaining,
    NullishCoalescing,
    /// An `<script type="module">` in the HTML — an engine-level feature the JS
    /// scan can't see (a module-loaded bundle may otherwise read as ES5).
    EsModule,
}

impl EsFeature {
    /// The minimum ES level this feature requires.
    pub fn level(self) -> EsLevel {
        match self {
            EsFeature::LetConst
            | EsFeature::Arrow
            | EsFeature::TemplateLiteral
            | EsFeature::Class
            | EsFeature::Spread => EsLevel::Es2015,
            EsFeature::Exponent => EsLevel::Es2016,
            EsFeature::AsyncAwait => EsLevel::Es2017,
            // ES modules ship in Chromium 61. The nearest EsLevel bucket floor
            // is Es2018 (Chromium 60); since no target firmware ships Chromium
            // 60–67 (the engine set jumps 53→68), this yields the same verdict
            // as requiring 61 exactly.
            EsFeature::EsModule => EsLevel::Es2018,
            EsFeature::OptionalChaining | EsFeature::NullishCoalescing => EsLevel::Es2020,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EsFeature::LetConst => "let/const",
            EsFeature::Arrow => "arrow function",
            EsFeature::TemplateLiteral => "template literal",
            EsFeature::Class => "class",
            EsFeature::Spread => "spread/rest",
            EsFeature::Exponent => "exponent (**)",
            EsFeature::AsyncAwait => "async/await",
            EsFeature::OptionalChaining => "optional chaining (?.)",
            EsFeature::NullishCoalescing => "nullish coalescing (??)",
            EsFeature::EsModule => "ES module (<script type=module>)",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_major_round_trips_to_level() {
        assert_eq!(EsLevel::from_chromium_major(120), EsLevel::Es2021Plus);
        assert_eq!(EsLevel::from_chromium_major(80), EsLevel::Es2020);
        assert_eq!(EsLevel::from_chromium_major(55), EsLevel::Es2017);
        assert_eq!(EsLevel::from_chromium_major(49), EsLevel::Es2015);
        // WebKit 537-era / Chromium 38 predate ES2015.
        assert_eq!(EsLevel::from_chromium_major(38), EsLevel::Es5);
        assert_eq!(EsLevel::from_chromium_major(0), EsLevel::Es5);
    }

    #[test]
    fn feature_levels_order() {
        assert!(EsFeature::OptionalChaining.level() > EsFeature::AsyncAwait.level());
        assert!(EsFeature::AsyncAwait.level() > EsFeature::Arrow.level());
    }
}
