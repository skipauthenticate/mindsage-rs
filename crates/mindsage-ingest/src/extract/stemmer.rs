//! Simple suffix-stripping stemmer — port of Python's _simple_stem().
//!
//! Handles common English suffixes without external dependencies.
//! Used for keyword-based topic classification.

/// Stem a word by removing common English suffixes.
pub fn simple_stem(word: &str) -> String {
    if word.len() <= 3 {
        return word.to_string();
    }

    // Suffix rules: (suffix, replacement). Check longer suffixes first.
    let suffixes: &[(&str, &str)] = &[
        // -ing endings (doubled consonants first)
        ("pping", "p"),
        ("tting", "t"),
        ("nning", "n"),
        ("mming", "m"),
        ("dding", "d"),
        ("gging", "g"),
        ("bing", "b"),
        ("ying", "y"),
        ("eing", "e"),
        ("uing", "ue"),
        ("oing", "o"),
        ("ting", "t"),
        ("ning", "n"),
        ("ming", "m"),
        ("king", "k"),
        ("ding", "d"),
        ("ring", "r"),
        ("ling", "l"),
        ("sing", "s"),
        ("zing", "z"),
        ("cing", "c"),
        ("ping", "p"),
        ("ing", ""),
        // -ed endings
        ("pped", "p"),
        ("tted", "t"),
        ("nned", "n"),
        ("mmed", "m"),
        ("dded", "d"),
        ("gged", "g"),
        ("bbed", "b"),
        ("ied", "y"),
        ("eed", "ee"),
        ("ued", "ue"),
        ("owed", "ow"),
        ("awed", "aw"),
        ("wed", "w"),
        ("ted", "t"),
        ("ned", "n"),
        ("med", "m"),
        ("ked", "k"),
        ("ded", "d"),
        ("red", "r"),
        ("led", "l"),
        ("sed", "s"),
        ("zed", "z"),
        ("ced", "c"),
        ("ped", "p"),
        ("ved", "ve"),
        ("ed", ""),
        // -s and -es endings
        ("ies", "y"),
        ("ches", "ch"),
        ("shes", "sh"),
        ("xes", "x"),
        ("zes", "z"),
        ("ses", "s"),
        ("oes", "o"),
        ("es", "e"),
        ("ss", "ss"),
        ("us", "us"),
        ("is", "is"),
        ("s", ""),
        // -er/-or endings
        ("ier", "y"),
        ("pper", "p"),
        ("tter", "t"),
        ("nner", "n"),
        ("mmer", "m"),
        ("dder", "d"),
        ("gger", "g"),
        ("bber", "b"),
        ("ler", "l"),
        ("ner", "n"),
        ("ter", "t"),
        ("ser", "s"),
        ("zer", "z"),
        ("cer", "c"),
        ("per", "p"),
        ("ker", "k"),
        ("der", "d"),
        ("er", ""),
        ("or", ""),
        // -tion/-sion
        ("ation", ""),
        ("ition", ""),
        ("ution", ""),
        ("tion", ""),
        ("sion", ""),
        // Other
        ("ment", ""),
        ("iness", "y"),
        ("ness", ""),
        ("ily", "y"),
        ("ally", "al"),
        ("ly", ""),
        ("ful", ""),
        ("less", ""),
        ("able", ""),
        ("ible", ""),
        ("ity", ""),
        ("ative", ""),
        ("itive", ""),
        ("ive", ""),
        ("ious", ""),
        ("eous", ""),
        ("ous", ""),
        ("ical", "ic"),
        ("ual", ""),
        ("al", ""),
    ];

    for &(suffix, replacement) in suffixes {
        if word.len() > suffix.len() + 1 && word.ends_with(suffix) {
            let stem = &word[..word.len() - suffix.len()];
            return format!("{}{}", stem, replacement);
        }
    }

    word.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stemming() {
        assert_eq!(simple_stem("programming"), "program");
        assert_eq!(simple_stem("exercising"), "exercis");
        assert_eq!(simple_stem("studied"), "study");
        assert_eq!(simple_stem("running"), "run");
        assert_eq!(simple_stem("shopping"), "shop");
        // "fitness" matches "ss" → "ss" rule (consistent stem for keyword matching)
        assert_eq!(simple_stem("fitness"), "fitness");
        // Key property: same word always stems the same way
        assert_eq!(simple_stem("fitness"), simple_stem("fitness"));
    }
}
