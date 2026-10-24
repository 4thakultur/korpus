//! The built-in exercises offered when building a routine.
//!
//! The list covers the common free-weight, cable, machine and body-weight
//! exercises of a gym. Names were written for Korpus in English, French and
//! Spanish using the terms gyms in those languages use (for example
//! "développé couché", "press de banca"); an English term is kept where it is
//! the usual name in that language too ("hip thrust", "face pull"). An
//! exercise that is missing can be added by the user (see
//! [`super::CustomExercise`]).

use crate::foods::normalize;
use crate::i18n::Lang;

/// The main muscle group an exercise trains, used to browse the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Muscle {
    Chest,
    Back,
    Shoulders,
    Biceps,
    Triceps,
    Forearms,
    Abs,
    Glutes,
    Quads,
    Hamstrings,
    Calves,
    FullBody,
    Cardio,
}

impl Muscle {
    pub const ALL: [Muscle; 13] = [
        Muscle::Chest,
        Muscle::Back,
        Muscle::Shoulders,
        Muscle::Biceps,
        Muscle::Triceps,
        Muscle::Forearms,
        Muscle::Abs,
        Muscle::Glutes,
        Muscle::Quads,
        Muscle::Hamstrings,
        Muscle::Calves,
        Muscle::FullBody,
        Muscle::Cardio,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Muscle::Chest => "chest",
            Muscle::Back => "back",
            Muscle::Shoulders => "shoulders",
            Muscle::Biceps => "biceps",
            Muscle::Triceps => "triceps",
            Muscle::Forearms => "forearms",
            Muscle::Abs => "abs",
            Muscle::Glutes => "glutes",
            Muscle::Quads => "quads",
            Muscle::Hamstrings => "hamstrings",
            Muscle::Calves => "calves",
            Muscle::FullBody => "full_body",
            Muscle::Cardio => "cardio",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == s)
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Muscle::Chest => lang.t("Chest"),
            Muscle::Back => lang.t("Back"),
            Muscle::Shoulders => lang.t("Shoulders"),
            Muscle::Biceps => lang.t("Biceps"),
            Muscle::Triceps => lang.t("Triceps"),
            Muscle::Forearms => lang.t("Forearms"),
            Muscle::Abs => lang.t("Abs"),
            Muscle::Glutes => lang.t("Glutes"),
            Muscle::Quads => lang.t("Quadriceps"),
            Muscle::Hamstrings => lang.t("Hamstrings"),
            Muscle::Calves => lang.t("Calves"),
            Muscle::FullBody => lang.t("Full body"),
            Muscle::Cardio => lang.t("Cardio"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Equipment {
    Barbell,
    Dumbbell,
    EzBar,
    Kettlebell,
    Cable,
    Machine,
    Smith,
    Bodyweight,
    Band,
    Other,
}

impl Equipment {
    pub const ALL: [Equipment; 10] = [
        Equipment::Barbell,
        Equipment::Dumbbell,
        Equipment::EzBar,
        Equipment::Kettlebell,
        Equipment::Cable,
        Equipment::Machine,
        Equipment::Smith,
        Equipment::Bodyweight,
        Equipment::Band,
        Equipment::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Equipment::Barbell => "barbell",
            Equipment::Dumbbell => "dumbbell",
            Equipment::EzBar => "ez_bar",
            Equipment::Kettlebell => "kettlebell",
            Equipment::Cable => "cable",
            Equipment::Machine => "machine",
            Equipment::Smith => "smith",
            Equipment::Bodyweight => "bodyweight",
            Equipment::Band => "band",
            Equipment::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == s)
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Equipment::Barbell => lang.t("Barbell"),
            Equipment::Dumbbell => lang.t("Dumbbells"),
            Equipment::EzBar => lang.t("EZ bar"),
            Equipment::Kettlebell => lang.t("Kettlebell"),
            Equipment::Cable => lang.t("Cable"),
            Equipment::Machine => lang.t("Machine"),
            Equipment::Smith => lang.t("Smith machine"),
            Equipment::Bodyweight => lang.t("Body weight"),
            Equipment::Band => lang.t("Resistance band"),
            Equipment::Other => lang.t("Other"),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Exercise {
    /// Stable identifier stored in routines; never changed or reused.
    pub key: &'static str,
    pub muscle: Muscle,
    pub equipment: Equipment,
    /// Sets are held for a time instead of counted in reps.
    pub timed: bool,
    /// English, French and Spanish names.
    names: [&'static str; 3],
}

impl Exercise {
    pub fn name(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => self.names[0],
            Lang::Fr => self.names[1],
            Lang::Es => self.names[2],
        }
    }

    /// Normalized names in every language, so "bench press" also finds
    /// "Développé couché" in a French interface.
    pub fn search_text(&self) -> String {
        self.names
            .iter()
            .map(|n| normalize(n))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub fn find(key: &str) -> Option<&'static Exercise> {
    EXERCISES.iter().find(|e| e.key == key)
}

/// Exercises sorted by name in the interface language.
pub fn sorted(lang: Lang) -> Vec<&'static Exercise> {
    let mut v: Vec<&Exercise> = EXERCISES.iter().collect();
    v.sort_by_cached_key(|e| normalize(e.name(lang)));
    v
}

use Equipment::*;
use Muscle::*;

const fn e(
    key: &'static str,
    muscle: Muscle,
    equipment: Equipment,
    en: &'static str,
    fr: &'static str,
    es: &'static str,
) -> Exercise {
    Exercise {
        key,
        muscle,
        equipment,
        timed: false,
        names: [en, fr, es],
    }
}

/// An exercise whose sets are held for a time.
const fn timed(
    key: &'static str,
    muscle: Muscle,
    equipment: Equipment,
    en: &'static str,
    fr: &'static str,
    es: &'static str,
) -> Exercise {
    Exercise {
        timed: true,
        ..e(key, muscle, equipment, en, fr, es)
    }
}

pub const EXERCISES: &[Exercise] = &[
    // Chest
    e(
        "barbell_bench_press",
        Chest,
        Barbell,
        "Barbell bench press",
        "Développé couché à la barre",
        "Press de banca con barra",
    ),
    e(
        "barbell_incline_bench_press",
        Chest,
        Barbell,
        "Incline barbell bench press",
        "Développé incliné à la barre",
        "Press inclinado con barra",
    ),
    e(
        "barbell_decline_bench_press",
        Chest,
        Barbell,
        "Decline barbell bench press",
        "Développé décliné à la barre",
        "Press declinado con barra",
    ),
    e(
        "dumbbell_bench_press",
        Chest,
        Dumbbell,
        "Dumbbell bench press",
        "Développé couché aux haltères",
        "Press de banca con mancuernas",
    ),
    e(
        "dumbbell_incline_bench_press",
        Chest,
        Dumbbell,
        "Incline dumbbell bench press",
        "Développé incliné aux haltères",
        "Press inclinado con mancuernas",
    ),
    e(
        "dumbbell_fly",
        Chest,
        Dumbbell,
        "Dumbbell fly",
        "Écarté couché aux haltères",
        "Aperturas con mancuernas",
    ),
    e(
        "dumbbell_incline_fly",
        Chest,
        Dumbbell,
        "Incline dumbbell fly",
        "Écarté incliné aux haltères",
        "Aperturas inclinadas con mancuernas",
    ),
    e(
        "dumbbell_pullover",
        Chest,
        Dumbbell,
        "Dumbbell pullover",
        "Pull-over à l'haltère",
        "Pullover con mancuerna",
    ),
    e(
        "cable_crossover",
        Chest,
        Cable,
        "Cable crossover",
        "Écarté à la poulie vis-à-vis",
        "Cruce de poleas",
    ),
    e(
        "pec_deck",
        Chest,
        Machine,
        "Pec deck (machine fly)",
        "Pec deck (butterfly)",
        "Contractora de pecho (pec deck)",
    ),
    e(
        "machine_chest_press",
        Chest,
        Machine,
        "Machine chest press",
        "Presse pectorale à la machine",
        "Press de pecho en máquina",
    ),
    e(
        "smith_bench_press",
        Chest,
        Smith,
        "Smith machine bench press",
        "Développé couché à la barre guidée",
        "Press de banca en máquina Smith",
    ),
    e(
        "push_up",
        Chest,
        Bodyweight,
        "Push-up",
        "Pompes",
        "Flexiones",
    ),
    e(
        "chest_dip",
        Chest,
        Bodyweight,
        "Chest dip",
        "Dips (pectoraux)",
        "Fondos en paralelas (pecho)",
    ),
    // Back
    e(
        "deadlift",
        Back,
        Barbell,
        "Deadlift",
        "Soulevé de terre",
        "Peso muerto",
    ),
    e(
        "pull_up",
        Back,
        Bodyweight,
        "Pull-up",
        "Tractions (prise pronation)",
        "Dominadas (agarre prono)",
    ),
    e(
        "chin_up",
        Back,
        Bodyweight,
        "Chin-up",
        "Tractions (prise supination)",
        "Dominadas (agarre supino)",
    ),
    e(
        "assisted_pull_up",
        Back,
        Machine,
        "Assisted pull-up",
        "Tractions assistées à la machine",
        "Dominadas asistidas en máquina",
    ),
    e(
        "lat_pulldown",
        Back,
        Cable,
        "Lat pulldown",
        "Tirage vertical à la poulie haute",
        "Jalón al pecho",
    ),
    e(
        "close_grip_lat_pulldown",
        Back,
        Cable,
        "Close-grip lat pulldown",
        "Tirage vertical prise serrée",
        "Jalón al pecho con agarre estrecho",
    ),
    e(
        "seated_cable_row",
        Back,
        Cable,
        "Seated cable row",
        "Tirage horizontal à la poulie basse",
        "Remo sentado en polea baja",
    ),
    e(
        "barbell_row",
        Back,
        Barbell,
        "Barbell bent-over row",
        "Rowing barre buste penché",
        "Remo inclinado con barra",
    ),
    e(
        "pendlay_row",
        Back,
        Barbell,
        "Pendlay row",
        "Rowing Pendlay",
        "Remo Pendlay",
    ),
    e(
        "dumbbell_row",
        Back,
        Dumbbell,
        "One-arm dumbbell row",
        "Rowing unilatéral à l'haltère",
        "Remo a una mano con mancuerna",
    ),
    e(
        "t_bar_row",
        Back,
        Barbell,
        "T-bar row",
        "Rowing T-bar",
        "Remo en barra T",
    ),
    e(
        "machine_row",
        Back,
        Machine,
        "Machine row",
        "Rowing assis à la machine",
        "Remo en máquina",
    ),
    e(
        "inverted_row",
        Back,
        Bodyweight,
        "Inverted row",
        "Rowing inversé",
        "Remo invertido",
    ),
    e(
        "straight_arm_pulldown",
        Back,
        Cable,
        "Straight-arm pulldown",
        "Pull-over à la poulie haute, bras tendus",
        "Jalón con brazos rectos en polea alta",
    ),
    e(
        "back_extension",
        Back,
        Bodyweight,
        "Back extension",
        "Extensions lombaires",
        "Hiperextensiones lumbares",
    ),
    e(
        "good_morning",
        Back,
        Barbell,
        "Good morning",
        "Good morning",
        "Buenos días con barra (good morning)",
    ),
    e(
        "rack_pull",
        Back,
        Barbell,
        "Rack pull",
        "Soulevé de terre partiel (rack pull)",
        "Peso muerto parcial desde soportes (rack pull)",
    ),
    e(
        "barbell_shrug",
        Back,
        Barbell,
        "Barbell shrug",
        "Shrugs à la barre",
        "Encogimientos de hombros con barra",
    ),
    e(
        "dumbbell_shrug",
        Back,
        Dumbbell,
        "Dumbbell shrug",
        "Shrugs aux haltères",
        "Encogimientos de hombros con mancuernas",
    ),
    // Shoulders
    e(
        "overhead_press",
        Shoulders,
        Barbell,
        "Overhead press",
        "Développé militaire",
        "Press militar",
    ),
    e(
        "seated_dumbbell_press",
        Shoulders,
        Dumbbell,
        "Seated dumbbell shoulder press",
        "Développé épaules assis aux haltères",
        "Press de hombros sentado con mancuernas",
    ),
    e(
        "standing_dumbbell_press",
        Shoulders,
        Dumbbell,
        "Standing dumbbell shoulder press",
        "Développé épaules debout aux haltères",
        "Press de hombros de pie con mancuernas",
    ),
    e(
        "arnold_press",
        Shoulders,
        Dumbbell,
        "Arnold press",
        "Développé Arnold",
        "Press Arnold",
    ),
    e(
        "machine_shoulder_press",
        Shoulders,
        Machine,
        "Machine shoulder press",
        "Développé épaules à la machine",
        "Press de hombros en máquina",
    ),
    e(
        "dumbbell_lateral_raise",
        Shoulders,
        Dumbbell,
        "Dumbbell lateral raise",
        "Élévations latérales aux haltères",
        "Elevaciones laterales con mancuernas",
    ),
    e(
        "cable_lateral_raise",
        Shoulders,
        Cable,
        "Cable lateral raise",
        "Élévation latérale à la poulie",
        "Elevación lateral en polea",
    ),
    e(
        "dumbbell_front_raise",
        Shoulders,
        Dumbbell,
        "Dumbbell front raise",
        "Élévations frontales aux haltères",
        "Elevaciones frontales con mancuernas",
    ),
    e(
        "rear_delt_fly",
        Shoulders,
        Dumbbell,
        "Bent-over rear delt fly",
        "Oiseau aux haltères",
        "Aperturas posteriores con mancuernas (pájaros)",
    ),
    e(
        "reverse_pec_deck",
        Shoulders,
        Machine,
        "Reverse pec deck",
        "Oiseau à la machine (pec deck inversé)",
        "Contractora inversa (deltoides posterior)",
    ),
    e(
        "face_pull",
        Shoulders,
        Cable,
        "Face pull",
        "Face pull à la poulie",
        "Face pull en polea",
    ),
    e(
        "upright_row",
        Shoulders,
        Barbell,
        "Barbell upright row",
        "Rowing menton à la barre",
        "Remo al mentón con barra",
    ),
    e(
        "pike_push_up",
        Shoulders,
        Bodyweight,
        "Pike push-up",
        "Pompes piquées",
        "Flexiones en pica",
    ),
    // Biceps
    e(
        "barbell_curl",
        Biceps,
        Barbell,
        "Barbell curl",
        "Curl à la barre",
        "Curl con barra",
    ),
    e(
        "ez_bar_curl",
        Biceps,
        EzBar,
        "EZ-bar curl",
        "Curl à la barre EZ",
        "Curl con barra Z",
    ),
    e(
        "dumbbell_curl",
        Biceps,
        Dumbbell,
        "Dumbbell curl",
        "Curl aux haltères",
        "Curl con mancuernas",
    ),
    e(
        "hammer_curl",
        Biceps,
        Dumbbell,
        "Hammer curl",
        "Curl marteau",
        "Curl martillo",
    ),
    e(
        "incline_dumbbell_curl",
        Biceps,
        Dumbbell,
        "Incline dumbbell curl",
        "Curl incliné aux haltères",
        "Curl inclinado con mancuernas",
    ),
    e(
        "preacher_curl",
        Biceps,
        EzBar,
        "Preacher curl",
        "Curl au pupitre",
        "Curl en banco Scott",
    ),
    e(
        "concentration_curl",
        Biceps,
        Dumbbell,
        "Concentration curl",
        "Curl concentré",
        "Curl concentrado",
    ),
    e(
        "cable_curl",
        Biceps,
        Cable,
        "Cable curl",
        "Curl à la poulie basse",
        "Curl en polea baja",
    ),
    e(
        "machine_bicep_curl",
        Biceps,
        Machine,
        "Machine biceps curl",
        "Curl biceps à la machine",
        "Curl de bíceps en máquina",
    ),
    // Triceps
    e(
        "triceps_pushdown",
        Triceps,
        Cable,
        "Triceps pushdown",
        "Extension des triceps à la poulie haute",
        "Extensión de tríceps en polea alta",
    ),
    e(
        "rope_pushdown",
        Triceps,
        Cable,
        "Rope triceps pushdown",
        "Extension des triceps à la poulie haute, corde",
        "Extensión de tríceps en polea alta con cuerda",
    ),
    e(
        "overhead_cable_extension",
        Triceps,
        Cable,
        "Overhead cable triceps extension",
        "Extension des triceps à la poulie, au-dessus de la tête",
        "Extensión de tríceps por encima de la cabeza en polea",
    ),
    e(
        "skull_crusher",
        Triceps,
        EzBar,
        "Skull crusher",
        "Barre au front",
        "Press francés",
    ),
    e(
        "overhead_dumbbell_extension",
        Triceps,
        Dumbbell,
        "Overhead dumbbell triceps extension",
        "Extension des triceps à l'haltère, au-dessus de la tête",
        "Extensión de tríceps por encima de la cabeza con mancuerna",
    ),
    e(
        "close_grip_bench_press",
        Triceps,
        Barbell,
        "Close-grip bench press",
        "Développé couché prise serrée",
        "Press de banca con agarre cerrado",
    ),
    e(
        "triceps_kickback",
        Triceps,
        Dumbbell,
        "Dumbbell triceps kickback",
        "Kickback triceps à l'haltère",
        "Patada de tríceps con mancuerna",
    ),
    e(
        "triceps_dip",
        Triceps,
        Bodyweight,
        "Triceps dip",
        "Dips (triceps)",
        "Fondos en paralelas (tríceps)",
    ),
    e(
        "bench_dip",
        Triceps,
        Bodyweight,
        "Bench dip",
        "Dips entre deux bancs",
        "Fondos entre bancos",
    ),
    e(
        "diamond_push_up",
        Triceps,
        Bodyweight,
        "Diamond push-up",
        "Pompes diamant",
        "Flexiones diamante",
    ),
    // Forearms
    e(
        "wrist_curl",
        Forearms,
        Barbell,
        "Barbell wrist curl",
        "Flexion des poignets à la barre",
        "Curl de muñeca con barra",
    ),
    e(
        "reverse_wrist_curl",
        Forearms,
        Barbell,
        "Reverse barbell wrist curl",
        "Extension des poignets à la barre",
        "Curl de muñeca inverso con barra",
    ),
    e(
        "reverse_curl",
        Forearms,
        Barbell,
        "Reverse barbell curl",
        "Curl inversé à la barre",
        "Curl inverso con barra",
    ),
    timed(
        "dead_hang",
        Forearms,
        Bodyweight,
        "Dead hang",
        "Suspension à la barre",
        "Suspensión en barra",
    ),
    // Abs
    timed(
        "plank",
        Abs,
        Bodyweight,
        "Plank",
        "Gainage (planche)",
        "Plancha abdominal",
    ),
    timed(
        "side_plank",
        Abs,
        Bodyweight,
        "Side plank",
        "Gainage latéral",
        "Plancha lateral",
    ),
    e(
        "crunch",
        Abs,
        Bodyweight,
        "Crunch",
        "Crunch",
        "Crunch abdominal",
    ),
    e(
        "sit_up",
        Abs,
        Bodyweight,
        "Sit-up",
        "Redressement assis",
        "Abdominales completos (sit-up)",
    ),
    e(
        "bicycle_crunch",
        Abs,
        Bodyweight,
        "Bicycle crunch",
        "Crunch vélo",
        "Crunch bicicleta",
    ),
    e(
        "cable_crunch",
        Abs,
        Cable,
        "Cable crunch",
        "Crunch à la poulie haute",
        "Crunch en polea alta",
    ),
    e(
        "hanging_leg_raise",
        Abs,
        Bodyweight,
        "Hanging leg raise",
        "Relevé de jambes suspendu",
        "Elevación de piernas colgado",
    ),
    e(
        "lying_leg_raise",
        Abs,
        Bodyweight,
        "Lying leg raise",
        "Relevé de jambes allongé",
        "Elevación de piernas tumbado",
    ),
    e(
        "russian_twist",
        Abs,
        Bodyweight,
        "Russian twist",
        "Rotations russes (russian twist)",
        "Giros rusos",
    ),
    e(
        "ab_wheel_rollout",
        Abs,
        Other,
        "Ab wheel rollout",
        "Roue abdominale",
        "Rueda abdominal",
    ),
    e(
        "dead_bug",
        Abs,
        Bodyweight,
        "Dead bug",
        "Dead bug",
        "Bicho muerto (dead bug)",
    ),
    e(
        "mountain_climber",
        Abs,
        Bodyweight,
        "Mountain climber",
        "Mountain climbers",
        "Escaladores (mountain climbers)",
    ),
    e(
        "pallof_press",
        Abs,
        Cable,
        "Pallof press",
        "Pallof press à la poulie",
        "Press Pallof en polea",
    ),
    // Glutes
    e(
        "hip_thrust",
        Glutes,
        Barbell,
        "Barbell hip thrust",
        "Hip thrust à la barre",
        "Hip thrust con barra",
    ),
    e(
        "glute_bridge",
        Glutes,
        Bodyweight,
        "Glute bridge",
        "Pont fessier",
        "Puente de glúteos",
    ),
    e(
        "cable_kickback",
        Glutes,
        Cable,
        "Cable glute kickback",
        "Kickback fessier à la poulie",
        "Patada de glúteo en polea",
    ),
    e(
        "hip_abduction",
        Glutes,
        Machine,
        "Hip abduction machine",
        "Abduction de hanche à la machine",
        "Abducción de cadera en máquina",
    ),
    e(
        "hip_adduction",
        Glutes,
        Machine,
        "Hip adduction machine",
        "Adduction de hanche à la machine",
        "Aducción de cadera en máquina",
    ),
    e(
        "sumo_deadlift",
        Glutes,
        Barbell,
        "Sumo deadlift",
        "Soulevé de terre sumo",
        "Peso muerto sumo",
    ),
    // Quadriceps
    e(
        "back_squat",
        Quads,
        Barbell,
        "Barbell back squat",
        "Squat à la barre",
        "Sentadilla trasera con barra",
    ),
    e(
        "front_squat",
        Quads,
        Barbell,
        "Front squat",
        "Squat avant (front squat)",
        "Sentadilla frontal con barra",
    ),
    e(
        "goblet_squat",
        Quads,
        Dumbbell,
        "Goblet squat",
        "Goblet squat",
        "Sentadilla goblet",
    ),
    e(
        "smith_squat",
        Quads,
        Smith,
        "Smith machine squat",
        "Squat à la barre guidée",
        "Sentadilla en máquina Smith",
    ),
    e(
        "leg_press",
        Quads,
        Machine,
        "Leg press",
        "Presse à cuisses",
        "Prensa de piernas",
    ),
    e(
        "hack_squat",
        Quads,
        Machine,
        "Hack squat",
        "Hack squat à la machine",
        "Sentadilla hack en máquina",
    ),
    e(
        "leg_extension",
        Quads,
        Machine,
        "Leg extension",
        "Leg extension à la machine",
        "Extensión de cuádriceps en máquina",
    ),
    e(
        "dumbbell_lunge",
        Quads,
        Dumbbell,
        "Dumbbell lunge",
        "Fentes avant aux haltères",
        "Zancadas con mancuernas",
    ),
    e(
        "walking_lunge",
        Quads,
        Dumbbell,
        "Walking lunge",
        "Fentes marchées",
        "Zancadas caminando",
    ),
    e(
        "bulgarian_split_squat",
        Quads,
        Dumbbell,
        "Bulgarian split squat",
        "Squat bulgare",
        "Sentadilla búlgara",
    ),
    e(
        "step_up",
        Quads,
        Dumbbell,
        "Dumbbell step-up",
        "Montées sur banc aux haltères",
        "Subidas al banco con mancuernas",
    ),
    e(
        "bodyweight_squat",
        Quads,
        Bodyweight,
        "Bodyweight squat",
        "Squat au poids du corps",
        "Sentadilla con peso corporal",
    ),
    timed(
        "wall_sit",
        Quads,
        Bodyweight,
        "Wall sit",
        "Chaise contre le mur",
        "Sentadilla isométrica en pared",
    ),
    // Hamstrings
    e(
        "romanian_deadlift",
        Hamstrings,
        Barbell,
        "Romanian deadlift",
        "Soulevé de terre roumain",
        "Peso muerto rumano",
    ),
    e(
        "dumbbell_romanian_deadlift",
        Hamstrings,
        Dumbbell,
        "Dumbbell Romanian deadlift",
        "Soulevé de terre roumain aux haltères",
        "Peso muerto rumano con mancuernas",
    ),
    e(
        "stiff_leg_deadlift",
        Hamstrings,
        Barbell,
        "Stiff-legged deadlift",
        "Soulevé de terre jambes tendues",
        "Peso muerto con piernas rígidas",
    ),
    e(
        "lying_leg_curl",
        Hamstrings,
        Machine,
        "Lying leg curl",
        "Leg curl allongé à la machine",
        "Curl femoral tumbado en máquina",
    ),
    e(
        "seated_leg_curl",
        Hamstrings,
        Machine,
        "Seated leg curl",
        "Leg curl assis à la machine",
        "Curl femoral sentado en máquina",
    ),
    e(
        "nordic_curl",
        Hamstrings,
        Bodyweight,
        "Nordic hamstring curl",
        "Nordic curl (ischio-jambiers)",
        "Curl nórdico de isquiotibiales",
    ),
    e(
        "kettlebell_swing",
        Hamstrings,
        Kettlebell,
        "Kettlebell swing",
        "Swing kettlebell",
        "Swing con kettlebell",
    ),
    // Calves
    e(
        "standing_calf_raise",
        Calves,
        Machine,
        "Standing calf raise",
        "Extension des mollets debout à la machine",
        "Elevación de gemelos de pie en máquina",
    ),
    e(
        "seated_calf_raise",
        Calves,
        Machine,
        "Seated calf raise",
        "Extension des mollets assis à la machine",
        "Elevación de gemelos sentado en máquina",
    ),
    e(
        "leg_press_calf_raise",
        Calves,
        Machine,
        "Calf press on leg press",
        "Extension des mollets à la presse",
        "Elevación de gemelos en prensa",
    ),
    e(
        "dumbbell_calf_raise",
        Calves,
        Dumbbell,
        "Dumbbell calf raise",
        "Extension des mollets aux haltères",
        "Elevación de gemelos con mancuernas",
    ),
    // Full body
    e(
        "power_clean",
        FullBody,
        Barbell,
        "Power clean",
        "Épaulé en force (power clean)",
        "Cargada de potencia (power clean)",
    ),
    e(
        "clean_and_jerk",
        FullBody,
        Barbell,
        "Clean and jerk",
        "Épaulé-jeté",
        "Dos tiempos",
    ),
    e(
        "snatch",
        FullBody,
        Barbell,
        "Snatch",
        "Arraché",
        "Arrancada",
    ),
    e(
        "thruster",
        FullBody,
        Barbell,
        "Barbell thruster",
        "Thruster à la barre",
        "Thruster con barra",
    ),
    e(
        "burpee", FullBody, Bodyweight, "Burpee", "Burpees", "Burpees",
    ),
    timed(
        "farmers_walk",
        FullBody,
        Dumbbell,
        "Farmer's walk",
        "Marche du fermier",
        "Paseo del granjero",
    ),
    e(
        "kettlebell_goblet_squat",
        FullBody,
        Kettlebell,
        "Kettlebell goblet squat",
        "Goblet squat au kettlebell",
        "Sentadilla goblet con kettlebell",
    ),
    e(
        "turkish_get_up",
        FullBody,
        Kettlebell,
        "Turkish get-up",
        "Relevé turc (Turkish get-up)",
        "Levantamiento turco (Turkish get-up)",
    ),
    e(
        "box_jump",
        FullBody,
        Other,
        "Box jump",
        "Sauts sur box",
        "Saltos al cajón",
    ),
    // Cardio
    timed(
        "treadmill",
        Cardio,
        Machine,
        "Treadmill",
        "Tapis de course",
        "Cinta de correr",
    ),
    timed(
        "stationary_bike",
        Cardio,
        Machine,
        "Stationary bike",
        "Vélo d'appartement",
        "Bicicleta estática",
    ),
    timed(
        "rowing_machine",
        Cardio,
        Machine,
        "Rowing machine",
        "Rameur",
        "Máquina de remo",
    ),
    timed(
        "elliptical",
        Cardio,
        Machine,
        "Elliptical trainer",
        "Vélo elliptique",
        "Elíptica",
    ),
    timed(
        "stair_climber",
        Cardio,
        Machine,
        "Stair climber",
        "Simulateur d'escalier",
        "Escaladora",
    ),
    timed(
        "jump_rope",
        Cardio,
        Other,
        "Jump rope",
        "Corde à sauter",
        "Saltar la cuerda (comba)",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn keys_and_names_are_unique() {
        let mut keys = HashSet::new();
        for ex in EXERCISES {
            assert!(keys.insert(ex.key), "duplicate key {}", ex.key);
            assert!(
                ex.key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
                "{}",
                ex.key
            );
            assert!(ex.key.len() <= 40);
        }
        for lang in Lang::ALL {
            let mut names = HashSet::new();
            for ex in EXERCISES {
                let n = ex.name(lang);
                assert!(!n.trim().is_empty() && n.chars().count() <= 80, "{n}");
                assert!(
                    names.insert(normalize(n)),
                    "duplicate {} name: {n}",
                    lang.code()
                );
                // Names are sentence case: the first letter is upper case.
                assert!(
                    n.chars().next().is_some_and(char::is_uppercase),
                    "{} name not capitalized: {n}",
                    lang.code()
                );
            }
        }
    }

    #[test]
    fn every_muscle_group_has_exercises() {
        for m in Muscle::ALL {
            assert!(EXERCISES.iter().any(|e| e.muscle == m), "{m:?}");
            assert_eq!(Muscle::parse(m.as_str()), Some(m));
        }
        for eq in Equipment::ALL {
            assert_eq!(Equipment::parse(eq.as_str()), Some(eq));
        }
    }

    #[test]
    fn names_follow_the_language() {
        let bench = find("barbell_bench_press").unwrap();
        assert_eq!(bench.name(Lang::En), "Barbell bench press");
        assert_eq!(bench.name(Lang::Fr), "Développé couché à la barre");
        assert_eq!(bench.name(Lang::Es), "Press de banca con barra");
        assert!(bench.search_text().contains("developpe couche"));
        assert!(bench.search_text().contains("bench press"));
        assert!(find("plank").unwrap().timed);
        assert!(find("nope").is_none());
    }

    #[test]
    fn sorted_follows_the_interface_language() {
        let fr = sorted(Lang::Fr);
        assert_eq!(fr.len(), EXERCISES.len());
        let names: Vec<String> = fr.iter().map(|e| normalize(e.name(Lang::Fr))).collect();
        assert!(names.windows(2).all(|w| w[0] <= w[1]));
    }
}
