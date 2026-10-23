//! Countries the user can choose in their profile. The country only decides
//! which local food composition table is searched (see `datasets`); it is
//! never sent anywhere.
//!
//! Names are written out per interface language here rather than in the
//! translation catalog: they are reference data, not interface text.

use crate::i18n::Lang;

/// (ISO 3166-1 alpha-2 code, English, French, Spanish).
const COUNTRIES: &[(&str, &str, &str, &str)] = &[
    ("AD", "Andorra", "Andorre", "Andorra"),
    ("AR", "Argentina", "Argentine", "Argentina"),
    ("AT", "Austria", "Autriche", "Austria"),
    ("AU", "Australia", "Australie", "Australia"),
    ("BE", "Belgium", "Belgique", "Bélgica"),
    ("BG", "Bulgaria", "Bulgarie", "Bulgaria"),
    ("BO", "Bolivia", "Bolivie", "Bolivia"),
    ("BR", "Brazil", "Brésil", "Brasil"),
    ("CA", "Canada", "Canada", "Canadá"),
    ("CH", "Switzerland", "Suisse", "Suiza"),
    ("CI", "Côte d'Ivoire", "Côte d'Ivoire", "Costa de Marfil"),
    ("CL", "Chile", "Chili", "Chile"),
    ("CM", "Cameroon", "Cameroun", "Camerún"),
    ("CN", "China", "Chine", "China"),
    ("CO", "Colombia", "Colombie", "Colombia"),
    ("CR", "Costa Rica", "Costa Rica", "Costa Rica"),
    ("CU", "Cuba", "Cuba", "Cuba"),
    ("CY", "Cyprus", "Chypre", "Chipre"),
    ("CZ", "Czechia", "Tchéquie", "Chequia"),
    ("DE", "Germany", "Allemagne", "Alemania"),
    ("DK", "Denmark", "Danemark", "Dinamarca"),
    (
        "DO",
        "Dominican Republic",
        "République dominicaine",
        "República Dominicana",
    ),
    ("DZ", "Algeria", "Algérie", "Argelia"),
    ("EC", "Ecuador", "Équateur", "Ecuador"),
    ("EE", "Estonia", "Estonie", "Estonia"),
    ("ES", "Spain", "Espagne", "España"),
    ("FI", "Finland", "Finlande", "Finlandia"),
    ("FR", "France", "France", "Francia"),
    ("GB", "United Kingdom", "Royaume-Uni", "Reino Unido"),
    ("GR", "Greece", "Grèce", "Grecia"),
    ("GT", "Guatemala", "Guatemala", "Guatemala"),
    ("HN", "Honduras", "Honduras", "Honduras"),
    ("HR", "Croatia", "Croatie", "Croacia"),
    ("HT", "Haiti", "Haïti", "Haití"),
    ("HU", "Hungary", "Hongrie", "Hungría"),
    ("IE", "Ireland", "Irlande", "Irlanda"),
    ("IL", "Israel", "Israël", "Israel"),
    ("IN", "India", "Inde", "India"),
    ("IS", "Iceland", "Islande", "Islandia"),
    ("IT", "Italy", "Italie", "Italia"),
    ("JP", "Japan", "Japon", "Japón"),
    ("KR", "South Korea", "Corée du Sud", "Corea del Sur"),
    ("LB", "Lebanon", "Liban", "Líbano"),
    ("LI", "Liechtenstein", "Liechtenstein", "Liechtenstein"),
    ("LT", "Lithuania", "Lituanie", "Lituania"),
    ("LU", "Luxembourg", "Luxembourg", "Luxemburgo"),
    ("LV", "Latvia", "Lettonie", "Letonia"),
    ("MA", "Morocco", "Maroc", "Marruecos"),
    ("MC", "Monaco", "Monaco", "Mónaco"),
    ("MG", "Madagascar", "Madagascar", "Madagascar"),
    ("ML", "Mali", "Mali", "Malí"),
    ("MT", "Malta", "Malte", "Malta"),
    ("MX", "Mexico", "Mexique", "México"),
    ("NI", "Nicaragua", "Nicaragua", "Nicaragua"),
    ("NL", "Netherlands", "Pays-Bas", "Países Bajos"),
    ("NO", "Norway", "Norvège", "Noruega"),
    ("NZ", "New Zealand", "Nouvelle-Zélande", "Nueva Zelanda"),
    ("PA", "Panama", "Panama", "Panamá"),
    ("PE", "Peru", "Pérou", "Perú"),
    ("PL", "Poland", "Pologne", "Polonia"),
    ("PR", "Puerto Rico", "Porto Rico", "Puerto Rico"),
    ("PT", "Portugal", "Portugal", "Portugal"),
    ("PY", "Paraguay", "Paraguay", "Paraguay"),
    ("RO", "Romania", "Roumanie", "Rumanía"),
    ("SE", "Sweden", "Suède", "Suecia"),
    ("SG", "Singapore", "Singapour", "Singapur"),
    ("SI", "Slovenia", "Slovénie", "Eslovenia"),
    ("SK", "Slovakia", "Slovaquie", "Eslovaquia"),
    ("SN", "Senegal", "Sénégal", "Senegal"),
    ("SV", "El Salvador", "Salvador", "El Salvador"),
    ("TN", "Tunisia", "Tunisie", "Túnez"),
    ("TR", "Türkiye", "Turquie", "Turquía"),
    ("UA", "Ukraine", "Ukraine", "Ucrania"),
    ("US", "United States", "États-Unis", "Estados Unidos"),
    ("UY", "Uruguay", "Uruguay", "Uruguay"),
    ("VE", "Venezuela", "Venezuela", "Venezuela"),
    ("ZA", "South Africa", "Afrique du Sud", "Sudáfrica"),
];

/// Countries whose food labels follow EU-style rules (energy per Regulation
/// (EU) No 1169/2011, carbohydrate without fibre). A European table matches
/// their labels better than a North American one.
const EUROPE: &[&str] = &[
    "AD", "AT", "BE", "BG", "CH", "CY", "CZ", "DE", "DK", "EE", "ES", "FI", "FR", "GB", "GR", "HR",
    "HU", "IE", "IS", "IT", "LI", "LT", "LU", "LV", "MC", "MT", "NL", "NO", "PL", "PT", "RO", "SE",
    "SI", "SK",
];

/// The canonical code if `code` is a listed country (case-insensitive).
pub fn parse(code: &str) -> Option<&'static str> {
    let code = code.trim();
    COUNTRIES
        .iter()
        .find(|(c, ..)| c.eq_ignore_ascii_case(code))
        .map(|(c, ..)| *c)
}

pub fn name(code: &str, lang: Lang) -> Option<&'static str> {
    COUNTRIES
        .iter()
        .find(|(c, ..)| *c == code)
        .map(|(_, en, fr, es)| match lang {
            Lang::En => *en,
            Lang::Fr => *fr,
            Lang::Es => *es,
        })
}

pub fn is_european(code: &str) -> bool {
    EUROPE.contains(&code)
}

pub struct CountryOption {
    pub code: &'static str,
    pub name: &'static str,
}

/// Every listed country, sorted by its name in `lang`.
pub fn options(lang: Lang) -> Vec<CountryOption> {
    let mut list: Vec<CountryOption> = COUNTRIES
        .iter()
        .filter_map(|(code, ..)| {
            Some(CountryOption {
                code,
                name: name(code, lang)?,
            })
        })
        .collect();
    list.sort_by_cached_key(|c| crate::foods::normalize(c.name));
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_upper_case_pairs() {
        let mut seen = std::collections::HashSet::new();
        for (code, ..) in COUNTRIES {
            assert!(seen.insert(*code), "duplicate {code}");
            assert!(code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase()));
        }
        for code in EUROPE {
            assert!(parse(code).is_some(), "{code} is not listed");
        }
    }

    #[test]
    fn parses_and_names() {
        assert_eq!(parse(" fr "), Some("FR"));
        assert_eq!(parse("XX"), None);
        assert_eq!(parse(""), None);
        assert_eq!(name("ES", Lang::Es), Some("España"));
        assert_eq!(name("ES", Lang::Fr), Some("Espagne"));
        assert!(is_european("DE") && !is_european("US"));
    }

    #[test]
    fn options_sort_by_localized_name_ignoring_accents() {
        let fr = options(Lang::Fr);
        let pos = |c: &str| fr.iter().position(|o| o.code == c).unwrap();
        // "États-Unis" sorts with E, before "France".
        assert!(pos("US") < pos("FR"));
        assert_eq!(fr.len(), COUNTRIES.len());
    }
}
