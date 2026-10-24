//! The sports offered when adding an activity — the workout types of Apple
//! Health — and the energy they are estimated to use.
//!
//! Each sport has a MET value (energy use as a multiple of resting) taken
//! from the general or moderate-effort entry of the Compendium of Physical
//! Activities, rounded. A sport not in the list is recorded as a custom
//! sport with an [`Intensity`] chosen by the user.

use crate::i18n::Lang;

#[derive(Debug, PartialEq)]
pub struct Sport {
    /// Stable identifier stored with an activity.
    pub key: &'static str,
    pub met: f64,
    /// English, French and Spanish names.
    names: [&'static str; 3],
}

impl Sport {
    pub fn name(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => self.names[0],
            Lang::Fr => self.names[1],
            Lang::Es => self.names[2],
        }
    }

    /// Id of the sport's icon in `static/sports.svg`.
    pub fn icon(&self) -> &'static str {
        match self.key {
            "american_football" => "ball-american-football",
            "archery" => "archery-arrow",
            "australian_football" => "rugby",
            "badminton" => "feather",
            "barre" => "stretching-2",
            "baseball" => "ball-baseball",
            "basketball" => "ball-basketball",
            "bowling" => "bowling",
            "boxing" => "karate",
            "cardio_dance" => "music",
            "climbing" => "mountain",
            "cooldown" => "heart-down",
            "core_training" => "exercise-ball",
            "cricket" => "cricket",
            "cross_country_skiing" => "ski-jumping",
            "cross_training" => "heartbeat",
            "curling" => "curling",
            "cycling" => "bike",
            "dance" => "music",
            "disc_sports" => "disc-golf",
            "downhill_skiing" => "ski-jumping",
            "elliptical" => "treadmill",
            "equestrian_sports" => "horse",
            "fencing" => "sword",
            "fishing" => "fish-hook",
            "fitness_gaming" => "device-gamepad-2",
            "flexibility" => "stretching",
            "functional_strength_training" => "dumbbell",
            "golf" => "golf",
            "gymnastics" => "gymnastics",
            "hand_cycling" => "bike",
            "handball" => "play-handball",
            "hiit" => "flame",
            "hiking" => "trekking",
            "hockey" => "ice-skating",
            "hunting" => "deer",
            "jump_rope" => "jump-rope",
            "kickboxing" => "karate",
            "lacrosse" => "play-handball",
            "martial_arts" => "karate",
            "mind_and_body" => "leaf",
            "mixed_cardio" => "heartbeat",
            "paddle_sports" => "kayak",
            "pickleball" => "ping-pong",
            "pilates" => "stretching-2",
            "play" => "balloon",
            "preparation_and_recovery" => "massage",
            "racquetball" => "ball-tennis",
            "rowing" => "kayak",
            "rugby" => "rugby",
            "running" => "run",
            "sailing" => "sailboat",
            "skating" => "roller-skating",
            "snow_sports" => "snowflake",
            "snowboarding" => "snowboarding",
            "soccer" => "ball-football",
            "social_dance" => "music",
            "softball" => "ball-baseball",
            "squash" => "ball-tennis",
            "stair_stepper" => "stairs-up",
            "stairs" => "stairs",
            "step_training" => "stairs-up",
            "surfing_sports" => "ripple",
            "swimming" => "swimming",
            "table_tennis" => "ping-pong",
            "tai_chi" => "yin-yang",
            "tennis" => "ball-tennis",
            "track_and_field" => "run-sprint",
            "traditional_strength_training" => "barbell",
            "underwater_diving" => "scuba-diving",
            "volleyball" => "ball-volleyball",
            "walking" => "walk",
            "water_fitness" => "pool",
            "water_polo" => "waterpolo",
            "water_sports" => "jetski",
            "wheelchair_run_pace" => "wheelchair",
            "wheelchair_walk_pace" => "wheelchair",
            "wrestling" => "karate",
            "yoga" => "yoga",
            _ => CUSTOM_ICON,
        }
    }
}

const fn s(
    key: &'static str,
    met: f64,
    en: &'static str,
    fr: &'static str,
    es: &'static str,
) -> Sport {
    Sport {
        key,
        met,
        names: [en, fr, es],
    }
}

/// Icon of a sport not in the list.
pub const CUSTOM_ICON: &str = "activity";

pub const SPORTS: &[Sport] = &[
    s(
        "american_football",
        8.0,
        "American Football",
        "Football américain",
        "Fútbol americano",
    ),
    s("archery", 4.3, "Archery", "Tir à l'arc", "Tiro con arco"),
    s(
        "australian_football",
        8.0,
        "Australian Football",
        "Football australien",
        "Fútbol australiano",
    ),
    s("badminton", 5.5, "Badminton", "Badminton", "Bádminton"),
    s("barre", 3.5, "Barre", "Barre", "Barre"),
    s("baseball", 5.0, "Baseball", "Baseball", "Béisbol"),
    s("basketball", 6.5, "Basketball", "Basketball", "Baloncesto"),
    s("bowling", 3.8, "Bowling", "Bowling", "Bolos"),
    s("boxing", 7.0, "Boxing", "Boxe", "Boxeo"),
    s(
        "cardio_dance",
        6.5,
        "Cardio Dance",
        "Danse cardio",
        "Baile cardio",
    ),
    s("climbing", 7.5, "Climbing", "Escalade", "Escalada"),
    s(
        "cooldown",
        2.5,
        "Cooldown",
        "Retour au calme",
        "Enfriamiento",
    ),
    s(
        "core_training",
        3.8,
        "Core Training",
        "Gainage",
        "Entrenamiento del core",
    ),
    s("cricket", 4.8, "Cricket", "Cricket", "Críquet"),
    s(
        "cross_country_skiing",
        9.0,
        "Cross Country Skiing",
        "Ski de fond",
        "Esquí de fondo",
    ),
    s(
        "cross_training",
        7.0,
        "Cross Training",
        "Cross training",
        "Entrenamiento cruzado",
    ),
    s("curling", 4.0, "Curling", "Curling", "Curling"),
    s("cycling", 7.5, "Cycling", "Cyclisme", "Ciclismo"),
    s("dance", 5.0, "Dance", "Danse", "Baile"),
    s(
        "disc_sports",
        4.0,
        "Disc Sports",
        "Sports de disque",
        "Deportes de disco",
    ),
    s(
        "downhill_skiing",
        5.3,
        "Downhill Skiing",
        "Ski alpin",
        "Esquí alpino",
    ),
    s("elliptical", 5.0, "Elliptical", "Elliptique", "Elíptica"),
    s(
        "equestrian_sports",
        5.5,
        "Equestrian Sports",
        "Sports équestres",
        "Deportes ecuestres",
    ),
    s("fencing", 6.0, "Fencing", "Escrime", "Esgrima"),
    s("fishing", 3.5, "Fishing", "Pêche", "Pesca"),
    s(
        "fitness_gaming",
        4.0,
        "Fitness Gaming",
        "Jeux de fitness",
        "Videojuegos de fitness",
    ),
    s(
        "flexibility",
        2.3,
        "Flexibility",
        "Souplesse",
        "Flexibilidad",
    ),
    s(
        "functional_strength_training",
        4.5,
        "Functional Strength Training",
        "Renforcement musculaire fonctionnel",
        "Entrenamiento de fuerza funcional",
    ),
    s("golf", 4.8, "Golf", "Golf", "Golf"),
    s("gymnastics", 3.8, "Gymnastics", "Gymnastique", "Gimnasia"),
    s("hand_cycling", 5.0, "Hand Cycling", "Handbike", "Handbike"),
    s("handball", 8.0, "Handball", "Handball", "Balonmano"),
    s(
        "hiit",
        8.0,
        "High Intensity Interval Training",
        "Entraînement fractionné de haute intensité",
        "Entrenamiento por intervalos de alta intensidad",
    ),
    s("hiking", 6.0, "Hiking", "Randonnée", "Senderismo"),
    s("hockey", 8.0, "Hockey", "Hockey", "Hockey"),
    s("hunting", 5.0, "Hunting", "Chasse", "Caza"),
    s(
        "jump_rope",
        10.0,
        "Jump Rope",
        "Corde à sauter",
        "Saltar a la comba",
    ),
    s("kickboxing", 7.5, "Kickboxing", "Kickboxing", "Kickboxing"),
    s("lacrosse", 8.0, "Lacrosse", "Crosse", "Lacrosse"),
    s(
        "martial_arts",
        7.0,
        "Martial Arts",
        "Arts martiaux",
        "Artes marciales",
    ),
    s(
        "mind_and_body",
        2.5,
        "Mind and Body",
        "Corps et esprit",
        "Cuerpo y mente",
    ),
    s(
        "mixed_cardio",
        6.0,
        "Mixed Cardio",
        "Cardio mixte",
        "Cardio mixto",
    ),
    s(
        "paddle_sports",
        5.0,
        "Paddle Sports",
        "Sports de pagaie",
        "Deportes de remo con pala",
    ),
    s("pickleball", 4.5, "Pickleball", "Pickleball", "Pickleball"),
    s("pilates", 3.0, "Pilates", "Pilates", "Pilates"),
    s("play", 4.0, "Play", "Jeux", "Juego"),
    s(
        "preparation_and_recovery",
        2.3,
        "Preparation and Recovery",
        "Préparation et récupération",
        "Preparación y recuperación",
    ),
    s(
        "racquetball",
        7.0,
        "Racquetball",
        "Racquetball",
        "Ráquetbol",
    ),
    s("rowing", 6.0, "Rowing", "Aviron", "Remo"),
    s("rugby", 7.5, "Rugby", "Rugby", "Rugby"),
    s("running", 9.0, "Running", "Course à pied", "Carrera"),
    s("sailing", 3.0, "Sailing", "Voile", "Vela"),
    s("skating", 7.0, "Skating", "Patinage", "Patinaje"),
    s(
        "snow_sports",
        5.0,
        "Snow Sports",
        "Sports de neige",
        "Deportes de nieve",
    ),
    s(
        "snowboarding",
        5.3,
        "Snowboarding",
        "Snowboard",
        "Snowboard",
    ),
    s("soccer", 7.0, "Soccer", "Football", "Fútbol"),
    s(
        "social_dance",
        4.5,
        "Social Dance",
        "Danse de salon",
        "Baile de salón",
    ),
    s("softball", 5.0, "Softball", "Softball", "Sóftbol"),
    s("squash", 7.3, "Squash", "Squash", "Squash"),
    s(
        "stair_stepper",
        8.0,
        "Stair Stepper",
        "Stepper",
        "Escaladora",
    ),
    s("stairs", 6.0, "Stairs", "Escaliers", "Escaleras"),
    s("step_training", 6.5, "Step Training", "Step", "Step"),
    s(
        "surfing_sports",
        3.0,
        "Surfing Sports",
        "Sports de glisse",
        "Deportes de surf",
    ),
    s("swimming", 6.0, "Swimming", "Natation", "Natación"),
    s(
        "table_tennis",
        4.0,
        "Table Tennis",
        "Tennis de table",
        "Tenis de mesa",
    ),
    s("tai_chi", 3.0, "Tai Chi", "Tai-chi", "Taichí"),
    s("tennis", 7.0, "Tennis", "Tennis", "Tenis"),
    s(
        "track_and_field",
        6.0,
        "Track and Field",
        "Athlétisme",
        "Atletismo",
    ),
    s(
        "traditional_strength_training",
        3.5,
        "Traditional Strength Training",
        "Musculation traditionnelle",
        "Entrenamiento de fuerza tradicional",
    ),
    s(
        "underwater_diving",
        7.0,
        "Underwater Diving",
        "Plongée sous-marine",
        "Buceo",
    ),
    s("volleyball", 4.0, "Volleyball", "Volley-ball", "Voleibol"),
    s("walking", 3.5, "Walking", "Marche", "Caminata"),
    s("water_fitness", 5.5, "Water Fitness", "Aquagym", "Aquagym"),
    s("water_polo", 10.0, "Water Polo", "Water-polo", "Waterpolo"),
    s(
        "water_sports",
        4.0,
        "Water Sports",
        "Sports nautiques",
        "Deportes acuáticos",
    ),
    s(
        "wheelchair_run_pace",
        6.0,
        "Wheelchair Run Pace",
        "Fauteuil roulant, allure de course",
        "Silla de ruedas, ritmo de carrera",
    ),
    s(
        "wheelchair_walk_pace",
        3.0,
        "Wheelchair Walk Pace",
        "Fauteuil roulant, allure de marche",
        "Silla de ruedas, ritmo de caminata",
    ),
    s("wrestling", 6.0, "Wrestling", "Lutte", "Lucha"),
    s("yoga", 2.5, "Yoga", "Yoga", "Yoga"),
];

pub fn find(key: &str) -> Option<&'static Sport> {
    SPORTS.iter().find(|s| s.key == key)
}

/// The sport whose name, in any language, is `name`, ignoring case and
/// accents. Used to recognise activities recorded before the list existed.
pub fn find_by_name(name: &str) -> Option<&'static Sport> {
    let name = fold(name.trim());
    SPORTS
        .iter()
        .find(|s| s.names.iter().any(|n| fold(n) == name))
}

/// The sports in alphabetical order of their names in `lang`.
pub fn sorted(lang: Lang) -> Vec<&'static Sport> {
    let mut v: Vec<&Sport> = SPORTS.iter().collect();
    v.sort_by_cached_key(|s| fold(s.name(lang)));
    v
}

/// Lower case without accents, so "Équitation" sorts among the E's.
fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => out.push('a'),
            'ç' => out.push('c'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'ñ' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' => out.push('u'),
            'œ' => out.push_str("oe"),
            c => out.push(c),
        }
    }
    out
}

/// How hard a custom sport is, for its energy estimate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intensity {
    Light,
    Moderate,
    Vigorous,
}

impl Intensity {
    pub const ALL: [Intensity; 3] = [Intensity::Light, Intensity::Moderate, Intensity::Vigorous];

    pub fn as_str(self) -> &'static str {
        match self {
            Intensity::Light => "light",
            Intensity::Moderate => "moderate",
            Intensity::Vigorous => "vigorous",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|i| i.as_str() == s)
    }

    /// The Compendium's usual bounds: light below 3 METs, vigorous from 6.
    pub fn met(self) -> f64 {
        match self {
            Intensity::Light => 2.5,
            Intensity::Moderate => 4.5,
            Intensity::Vigorous => 7.5,
        }
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Intensity::Light => lang.t("Easy"),
            Intensity::Moderate => lang.t("Moderate"),
            Intensity::Vigorous => lang.t("Hard"),
        }
    }
}

/// Approximate energy used by `minutes` of an activity of `met` METs,
/// above what the body uses at rest (1 MET ≈ 1 kcal per kg per hour), since
/// resting energy is already part of the daily target.
pub fn net_kcal(met: f64, minutes: i64, weight_kg: f64) -> f64 {
    (met - 1.0).max(0.0) * weight_kg * minutes as f64 / 60.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn keys_are_unique_and_values_plausible() {
        let keys: HashSet<&str> = SPORTS.iter().map(|s| s.key).collect();
        assert_eq!(keys.len(), SPORTS.len());
        for s in SPORTS {
            assert!((2.0..=12.0).contains(&s.met), "{}", s.key);
            assert!(s.names.iter().all(|n| !n.is_empty()), "{}", s.key);
        }
    }

    #[test]
    fn every_sport_has_an_icon_in_the_sprite() {
        let sprite = include_str!("../static/sports.svg");
        let has = |id: &str| sprite.contains(&format!("<symbol id=\"{id}\""));
        assert!(has(CUSTOM_ICON));
        for s in SPORTS {
            assert_ne!(s.icon(), CUSTOM_ICON, "{}", s.key);
            assert!(has(s.icon()), "{}", s.key);
        }
    }

    #[test]
    fn sorted_by_localized_name_ignoring_accents() {
        let fr: Vec<&str> = sorted(Lang::Fr).iter().map(|s| s.name(Lang::Fr)).collect();
        let pos = |n: &str| fr.iter().position(|x| *x == n).unwrap();
        assert!(pos("Elliptique") < pos("Entraînement fractionné de haute intensité"));
        assert!(pos("Entraînement fractionné de haute intensité") < pos("Escalade"));
        assert_eq!(sorted(Lang::En)[0].key, "american_football");
    }

    #[test]
    fn names_are_found_in_any_language() {
        assert_eq!(find_by_name("course à pied").unwrap().key, "running");
        assert_eq!(find_by_name(" NATACION ").unwrap().key, "swimming");
        assert_eq!(find_by_name("Climbing").unwrap().key, "climbing");
        assert!(find_by_name("Quidditch").is_none());
        assert_eq!(find("yoga").unwrap().name(Lang::Es), "Yoga");
        assert!(find("custom").is_none());
    }

    #[test]
    fn energy_is_net_of_rest_and_scales_with_time_and_weight() {
        // 30 min of running (9 METs) at 70 kg: 8 × 70 × 0.5 = 280 kcal.
        assert!((net_kcal(9.0, 30, 70.0) - 280.0).abs() < 1e-9);
        assert_eq!(net_kcal(9.0, 60, 70.0), 2.0 * net_kcal(9.0, 30, 70.0));
        assert!(net_kcal(9.0, 30, 80.0) > net_kcal(9.0, 30, 70.0));
        assert_eq!(net_kcal(0.5, 30, 70.0), 0.0);
    }

    #[test]
    fn intensities_round_trip() {
        for i in Intensity::ALL {
            assert_eq!(Intensity::parse(i.as_str()), Some(i));
        }
        assert_eq!(Intensity::parse("extreme"), None);
        assert!(Intensity::Light.met() < Intensity::Moderate.met());
        assert!(Intensity::Moderate.met() < Intensity::Vigorous.met());
    }
}
