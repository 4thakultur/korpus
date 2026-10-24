//! Interface language and translations.
//!
//! English source strings are the keys: `lang.t("Diary")` returns the French
//! or Spanish text from [`CATALOG`], or the key itself for English. A test
//! checks that every key used in `src/` and `templates/` has a translation.
//!
//! Only the interface is translated. Food names, brands and notes are data
//! and are shown as stored; providers are asked for product names in the
//! interface language instead (see `providers`).

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Lang {
    #[default]
    En,
    Fr,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::En, Lang::Fr, Lang::Es];

    /// ISO 639-1 code, used for `<html lang>`, storage and provider requests.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
            Lang::Es => "es",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Lang::ALL.into_iter().find(|l| l.code() == s)
    }

    /// The language's own name, so it can be recognized in any interface language.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Fr => "Français",
            Lang::Es => "Español",
        }
    }

    /// Picks the best supported language from an `Accept-Language` header,
    /// falling back to English.
    pub fn from_accept_language(header: &str) -> Lang {
        let mut best: Option<(Lang, f32)> = None;
        for part in header.split(',') {
            let mut pieces = part.split(';');
            let tag = pieces.next().unwrap_or("").trim();
            let primary = tag.split('-').next().unwrap_or("").to_ascii_lowercase();
            let Some(lang) = Lang::parse(&primary) else {
                continue;
            };
            let q = pieces
                .find_map(|p| p.trim().strip_prefix("q="))
                .and_then(|q| q.trim().parse::<f32>().ok())
                .unwrap_or(1.0);
            if q > 0.0 && best.is_none_or(|(_, bq)| q > bq) {
                best = Some((lang, q));
            }
        }
        best.map(|(l, _)| l).unwrap_or_default()
    }

    /// Translates an English source string.
    pub fn t(self, en: &'static str) -> &'static str {
        if self == Lang::En {
            return en;
        }
        match catalog().get(en) {
            Some((fr, es)) => match self {
                Lang::Fr => fr,
                _ => es,
            },
            None => en,
        }
    }

    /// Translates `en` and replaces `{0}`, `{1}`, … with `args`.
    pub fn f(self, en: &'static str, args: &[&dyn fmt::Display]) -> String {
        let mut out = self.t(en).to_string();
        for (i, a) in args.iter().enumerate() {
            out = out.replace(&format!("{{{i}}}"), &a.to_string());
        }
        out
    }

    /// [`Lang::f`] with one argument, for templates (Askama passes arguments
    /// by reference, which does not coerce to a `&dyn Display` slice).
    pub fn f1(self, en: &'static str, a: impl fmt::Display) -> String {
        self.f(en, &[&a])
    }

    /// [`Lang::f`] with two arguments, for templates.
    pub fn f2(self, en: &'static str, a: impl fmt::Display, b: impl fmt::Display) -> String {
        self.f(en, &[&a, &b])
    }

    /// Localizes the decimal separator of an already formatted number.
    pub fn num(self, formatted: String) -> String {
        match self {
            Lang::En => formatted,
            Lang::Fr | Lang::Es => formatted.replace('.', ","),
        }
    }

    pub fn decimal_separator(self) -> &'static str {
        match self {
            Lang::En => ".",
            Lang::Fr | Lang::Es => ",",
        }
    }
}

fn catalog() -> &'static HashMap<&'static str, (&'static str, &'static str)> {
    static MAP: OnceLock<HashMap<&'static str, (&'static str, &'static str)>> = OnceLock::new();
    MAP.get_or_init(|| {
        CATALOG
            .iter()
            .map(|(en, fr, es)| (*en, (*fr, *es)))
            .collect()
    })
}

/// (English, French, Spanish).
pub const CATALOG: &[(&str, &str, &str)] = &[
    ("Breakfast", "Petit-déjeuner", "Desayuno"),
    ("Lunch", "Déjeuner", "Comida"),
    ("Dinner", "Dîner", "Cena"),
    ("Snacks", "Collations", "Tentempiés"),
    (
        "Add to breakfast",
        "Ajouter au petit-déjeuner",
        "Añadir al desayuno",
    ),
    ("Add to lunch", "Ajouter au déjeuner", "Añadir a la comida"),
    ("Add to dinner", "Ajouter au dîner", "Añadir a la cena"),
    (
        "Add to snacks",
        "Ajouter aux collations",
        "Añadir a los tentempiés",
    ),
    (
        "Add food to breakfast",
        "Ajouter un aliment au petit-déjeuner",
        "Añadir un alimento al desayuno",
    ),
    (
        "Add food to lunch",
        "Ajouter un aliment au déjeuner",
        "Añadir un alimento a la comida",
    ),
    (
        "Add food to dinner",
        "Ajouter un aliment au dîner",
        "Añadir un alimento a la cena",
    ),
    (
        "Add food to snacks",
        "Ajouter un aliment aux collations",
        "Añadir un alimento a los tentempiés",
    ),
    (
        "Enter a number.",
        "Saisissez un nombre.",
        "Introduzca un número.",
    ),
    (
        "Amount must be greater than zero.",
        "La quantité doit être supérieure à zéro.",
        "La cantidad debe ser mayor que cero.",
    ),
    (
        "Amount must be at most {0}.",
        "La quantité ne peut pas dépasser {0}.",
        "La cantidad no puede superar {0}.",
    ),
    ("Created by you", "Créé par vous", "Creado por usted"),
    ("Other source", "Autre source", "Otra fuente"),
    (
        "Enter a name.",
        "Saisissez un nom.",
        "Introduzca un nombre.",
    ),
    ("Name", "Nom", "Nombre"),
    ("Brand", "Marque", "Marca"),
    (
        "A barcode has 6 to 14 digits.",
        "Un code-barres comporte de 6 à 14 chiffres.",
        "Un código de barras tiene entre 6 y 14 dígitos.",
    ),
    (
        "Choose whether values are per 100 g or per 100 ml.",
        "Indiquez si les valeurs sont pour 100 g ou pour 100 ml.",
        "Indique si los valores son por 100 g o por 100 ml.",
    ),
    (
        "Choose kcal or kJ.",
        "Choisissez kcal ou kJ.",
        "Elija kcal o kJ.",
    ),
    (
        "Energy cannot be negative.",
        "L'énergie ne peut pas être négative.",
        "La energía no puede ser negativa.",
    ),
    (
        "Serving size must be between 0 and 10000.",
        "La taille de portion doit être comprise entre 0 et 10000.",
        "El tamaño de la ración debe estar entre 0 y 10000.",
    ),
    ("Serving size", "Taille de portion", "Tamaño de la ración"),
    ("Serving name", "Nom de la portion", "Nombre de la ración"),
    ("Notes", "Notes", "Notas"),
    (
        "Enter {0} (use 0 if none).",
        "Saisissez une valeur pour {0} (0 si aucun).",
        "Introduzca un valor para {0} (0 si no hay).",
    ),
    (
        "{0} {1} ({2} {3}).",
        "{0} : {1} ({2} {3}).",
        "{0}: {1} ({2} {3}).",
    ),
    (
        "Sedentary — little or no exercise (×1.2)",
        "Sédentaire — peu ou pas d'exercice (×1,2)",
        "Sedentario — poco o ningún ejercicio (×1,2)",
    ),
    (
        "Lightly active — light exercise 1–3 days/week (×1.375)",
        "Légèrement actif — exercice léger 1 à 3 jours/semaine (×1,375)",
        "Ligeramente activo — ejercicio ligero 1–3 días/semana (×1,375)",
    ),
    (
        "Moderately active — exercise 3–5 days/week (×1.55)",
        "Modérément actif — exercice 3 à 5 jours/semaine (×1,55)",
        "Moderadamente activo — ejercicio 3–5 días/semana (×1,55)",
    ),
    (
        "Very active — hard exercise 6–7 days/week (×1.725)",
        "Très actif — exercice intense 6 à 7 jours/semaine (×1,725)",
        "Muy activo — ejercicio intenso 6–7 días/semana (×1,725)",
    ),
    (
        "Extra active — physical job or training twice a day (×1.9)",
        "Extrêmement actif — travail physique ou entraînement deux fois par jour (×1,9)",
        "Extremadamente activo — trabajo físico o entrenamiento dos veces al día (×1,9)",
    ),
    ("Maintain weight", "Maintenir le poids", "Mantener el peso"),
    (
        "Lose slowly (about −250 kcal/day)",
        "Perdre lentement (environ −250 kcal/jour)",
        "Perder despacio (unas −250 kcal/día)",
    ),
    (
        "Lose (about −500 kcal/day)",
        "Perdre (environ −500 kcal/jour)",
        "Perder (unas −500 kcal/día)",
    ),
    (
        "Gain slowly (about +250 kcal/day)",
        "Prendre lentement (environ +250 kcal/jour)",
        "Ganar despacio (unas +250 kcal/día)",
    ),
    (
        "Because you indicated pregnancy, breastfeeding or a medical condition, Korpus does not suggest a weight-loss target. Please ask a qualified health professional for guidance.",
        "Comme vous avez indiqué une grossesse, un allaitement ou un problème de santé, Korpus ne propose pas d'objectif de perte de poids. Demandez conseil à un professionnel de santé qualifié.",
        "Como indicó embarazo, lactancia o una afección médica, Korpus no sugiere un objetivo de pérdida de peso. Consulte a un profesional sanitario cualificado.",
    ),
    (
        "The suggested target was limited so it does not fall below your estimated resting energy needs or 1200 kcal.",
        "L'objectif suggéré a été limité pour ne pas descendre sous vos besoins énergétiques au repos estimés ni sous 1200 kcal.",
        "El objetivo sugerido se limitó para que no quede por debajo de sus necesidades energéticas en reposo estimadas ni de 1200 kcal.",
    ),
    (
        "Enter a target between {0} and {1} kcal.",
        "Saisissez un objectif entre {0} et {1} kcal.",
        "Introduzca un objetivo entre {0} y {1} kcal.",
    ),
    ("Not found", "Introuvable", "No encontrado"),
    (
        "The page or item you were looking for does not exist. It may have been deleted.",
        "La page ou l'élément recherché n'existe pas. Il a peut-être été supprimé.",
        "La página o el elemento que busca no existe. Puede que se haya eliminado.",
    ),
    ("Invalid request", "Requête invalide", "Solicitud no válida"),
    ("Request blocked", "Requête bloquée", "Solicitud bloqueada"),
    (
        "Something went wrong",
        "Une erreur s'est produite",
        "Algo salió mal",
    ),
    (
        "Korpus could not complete this request. Your data was not changed unless a confirmation was shown. Check the server log for details.",
        "Korpus n'a pas pu traiter cette requête. Vos données n'ont pas été modifiées, sauf si une confirmation a été affichée. Consultez le journal du serveur pour plus de détails.",
        "Korpus no pudo completar esta solicitud. Sus datos no se modificaron salvo que se mostrara una confirmación. Consulte el registro del servidor para más detalles.",
    ),
    (
        "must be a number",
        "doit être un nombre",
        "debe ser un número",
    ),
    ("is too long", "est trop long", "es demasiado largo"),
    (
        "must be at most {0} characters",
        "ne doit pas dépasser {0} caractères",
        "no debe superar los {0} caracteres",
    ),
    ("{0} {1}.", "{0} : {1}.", "{0}: {1}."),
    (
        "this is not a Korpus export",
        "ce n'est pas un export Korpus",
        "no es una exportación de Korpus",
    ),
    (
        "unknown unit system",
        "système d'unités inconnu",
        "sistema de unidades desconocido",
    ),
    (
        "unknown time zone",
        "fuseau horaire inconnu",
        "zona horaria desconocida",
    ),
    ("unknown language", "langue inconnue", "idioma desconocido"),
    ("basis", "base", "base"),
    ("name", "nom", "nombre"),
    ("date", "date", "fecha"),
    ("meal", "repas", "comida"),
    ("unit", "unité", "unidad"),
    ("text too long", "texte trop long", "texto demasiado largo"),
    (
        "goal settings",
        "réglages d'objectif",
        "ajustes de objetivo",
    ),
    (
        "Import is only possible into an empty installation. This one already contains foods, diary entries, weights, activity or gym programs.",
        "L'import n'est possible que dans une installation vide. Celle-ci contient déjà des aliments, des entrées de journal, des poids, de l'activité ou des programmes de muscu.",
        "La importación solo es posible en una instalación vacía. Esta ya contiene alimentos, entradas del diario, pesos, actividad o programas de gimnasio.",
    ),
    (
        "The import file is invalid: {0}.",
        "Le fichier d'import est invalide : {0}.",
        "El archivo de importación no es válido: {0}.",
    ),
    (
        "unsupported export version {0}",
        "version d'export {0} non prise en charge",
        "versión de exportación {0} no compatible",
    ),
    ("food {0}", "aliment {0}", "alimento {0}"),
    (
        "diary entry {0}",
        "entrée de journal {0}",
        "entrada del diario {0}",
    ),
    ("weight {0}", "poids {0}", "peso {0}"),
    (
        "food {0} could not be stored ({1})",
        "l'aliment {0} n'a pas pu être enregistré ({1})",
        "no se pudo guardar el alimento {0} ({1})",
    ),
    (
        "weight for {0} could not be stored ({1})",
        "le poids du {0} n'a pas pu être enregistré ({1})",
        "no se pudo guardar el peso del {0} ({1})",
    ),
    ("per 100 g", "pour 100 g", "por 100 g"),
    ("per 100 ml", "pour 100 ml", "por 100 ml"),
    ("Energy", "Énergie", "Energía"),
    ("Protein", "Protéines", "Proteínas"),
    ("Carbohydrate", "Glucides", "Carbohidratos"),
    ("Fat", "Lipides", "Grasas"),
    ("Fibre", "Fibres", "Fibra"),
    ("Sugars", "Sucres", "Azúcares"),
    ("Saturated fat", "Acides gras saturés", "Grasas saturadas"),
    ("Salt", "Sel", "Sal"),
    (
        "cannot be negative",
        "ne peut pas être négatif",
        "no puede ser negativo",
    ),
    (
        "must not exceed {0}",
        "ne doit pas dépasser {0}",
        "no debe superar {0}",
    ),
    (
        "The date must be a valid calendar date (YYYY-MM-DD).",
        "La date doit être une date valide (AAAA-MM-JJ).",
        "La fecha debe ser una fecha válida (AAAA-MM-DD).",
    ),
    ("Today", "Aujourd'hui", "Hoy"),
    ("Yesterday", "Hier", "Ayer"),
    ("Tomorrow", "Demain", "Mañana"),
    ("Choose a date", "Choisir une date", "Elegir una fecha"),
    ("Previous month", "Mois précédent", "Mes anterior"),
    ("Next month", "Mois suivant", "Mes siguiente"),
    (
        "Choose a valid date.",
        "Choisissez une date valide.",
        "Elija una fecha válida.",
    ),
    (
        "Choose a meal.",
        "Choisissez un repas.",
        "Elija una comida.",
    ),
    (
        "Enter the amount you ate.",
        "Saisissez la quantité consommée.",
        "Introduzca la cantidad que comió.",
    ),
    ("Amount", "Quantité", "Cantidad"),
    ("Edit {0}", "Modifier {0}", "Editar {0}"),
    ("Not available", "Non disponible", "No disponible"),
    (
        "Product data © Open Food Facts contributors (ODbL).",
        "Données produit © contributeurs d'Open Food Facts (ODbL).",
        "Datos del producto © colaboradores de Open Food Facts (ODbL).",
    ),
    (
        "Product data © Open Food Facts contributors, available under the Open Database License (ODbL).",
        "Données produit © contributeurs d'Open Food Facts, disponibles sous licence Open Database License (ODbL).",
        "Datos del producto © colaboradores de Open Food Facts, disponibles bajo la Open Database License (ODbL).",
    ),
    ("of which sugars", "dont sucres", "de los cuales azúcares"),
    (
        "of which saturated fat",
        "dont acides gras saturés",
        "de las cuales saturadas",
    ),
    ("Create a food", "Créer un aliment", "Crear un alimento"),
    (
        "Another food already uses this barcode.",
        "Un autre aliment utilise déjà ce code-barres.",
        "Otro alimento ya usa este código de barras.",
    ),
    (
        "This food source is not enabled.",
        "Cette source d'aliments n'est pas activée.",
        "Esta fuente de alimentos no está activada.",
    ),
    (
        "Invalid product identifier.",
        "Identifiant de produit invalide.",
        "Identificador de producto no válido.",
    ),
    (
        "Product not found",
        "Produit introuvable",
        "Producto no encontrado",
    ),
    (
        "Create food manually",
        "Créer l'aliment manuellement",
        "Crear el alimento manualmente",
    ),
    (
        "Back to search",
        "Retour à la recherche",
        "Volver a la búsqueda",
    ),
    ("Per serving ({0})", "Par portion ({0})", "Por ración ({0})"),
    ("Delete {0}", "Supprimer {0}", "Eliminar {0}"),
    (
        "{0} is unavailable",
        "{0} est indisponible",
        "{0} no está disponible",
    ),
    (
        "{0} no longer has this product. You can create it manually.",
        "{0} ne propose plus ce produit. Vous pouvez le créer manuellement.",
        "{0} ya no tiene este producto. Puede crearlo manualmente.",
    ),
    (
        "The product could not be loaded: {0}. Your diary and saved foods are not affected. You can try again or create the food manually.",
        "Le produit n'a pas pu être chargé : {0}. Votre journal et vos aliments enregistrés ne sont pas affectés. Vous pouvez réessayer ou créer l'aliment manuellement.",
        "No se pudo cargar el producto: {0}. Su diario y sus alimentos guardados no se ven afectados. Puede intentarlo de nuevo o crear el alimento manualmente.",
    ),
    (
        "This food was imported. Your changes are kept locally and will not be overwritten when the product is imported again.",
        "Cet aliment a été importé. Vos modifications sont conservées localement et ne seront pas écrasées si le produit est importé à nouveau.",
        "Este alimento se importó. Sus cambios se guardan localmente y no se sobrescribirán si el producto se vuelve a importar.",
    ),
    (
        "Values are {0}; the basis was inferred from the product's package quantity.",
        "Les valeurs sont données {0} ; la base a été déduite de la quantité de l'emballage.",
        "Los valores son {0}; la base se dedujo de la cantidad del envase.",
    ),
    (
        "Invalid source values were ignored for: {0}.",
        "Des valeurs source invalides ont été ignorées pour : {0}.",
        "Se ignoraron valores de origen no válidos para: {0}.",
    ),
    (
        "Choose metric or imperial units.",
        "Choisissez les unités métriques ou impériales.",
        "Elija unidades métricas o imperiales.",
    ),
    (
        "Enter a time zone name such as Europe/Berlin, America/New_York or UTC.",
        "Saisissez un fuseau horaire, par exemple Europe/Paris, America/New_York ou UTC.",
        "Introduzca una zona horaria, por ejemplo Europe/Madrid, America/New_York o UTC.",
    ),
    (
        "Choose a language.",
        "Choisissez une langue.",
        "Elija un idioma.",
    ),
    (
        "Same as browser",
        "Comme le navigateur",
        "Igual que el navegador",
    ),
    (
        "Welcome to Korpus",
        "Bienvenue dans Korpus",
        "Bienvenido a Korpus",
    ),
    ("Settings", "Paramètres", "Ajustes"),
    ("Preferences", "Préférences", "Preferencias"),
    (
        "The upload could not be read.",
        "Le fichier envoyé n'a pas pu être lu.",
        "No se pudo leer el archivo enviado.",
    ),
    (
        "The upload could not be read (is it too large?).",
        "Le fichier envoyé n'a pas pu être lu (est-il trop volumineux ?).",
        "No se pudo leer el archivo enviado (¿es demasiado grande?).",
    ),
    (
        "Choose an export file to import.",
        "Choisissez un fichier d'export à importer.",
        "Elija un archivo de exportación para importar.",
    ),
    (
        "The file is too large.",
        "Le fichier est trop volumineux.",
        "El archivo es demasiado grande.",
    ),
    (
        "The file is not a valid Korpus JSON export.",
        "Ce fichier n'est pas un export JSON Korpus valide.",
        "El archivo no es una exportación JSON válida de Korpus.",
    ),
    (
        "Choose an option.",
        "Choisissez une option.",
        "Elija una opción.",
    ),
    (
        "Enter a valid date of birth.",
        "Saisissez une date de naissance valide.",
        "Introduzca una fecha de nacimiento válida.",
    ),
    ("This value", "Cette valeur", "Este valor"),
    (
        "Enter a height between 100 and 250 cm (39–98 in).",
        "Saisissez une taille entre 100 et 250 cm (39–98 in).",
        "Introduzca una altura entre 100 y 250 cm (39–98 in).",
    ),
    (
        "Enter a weight between 20 and 400 kg (44–880 lb).",
        "Saisissez un poids entre 20 et 400 kg (44–880 lb).",
        "Introduzca un peso entre 20 y 400 kg (44–880 lb).",
    ),
    (
        "Choose an activity level.",
        "Choisissez un niveau d'activité.",
        "Elija un nivel de actividad.",
    ),
    (
        "Enter a value between 0 and 1000 g.",
        "Saisissez une valeur entre 0 et 1000 g.",
        "Introduzca un valor entre 0 y 1000 g.",
    ),
    ("estimated", "estimé", "estimado"),
    ("set by you", "défini par vous", "definido por usted"),
    ("Goals", "Objectifs", "Objetivos"),
    (
        "Korpus does not estimate calorie targets for people under 18. Please ask a doctor or registered dietitian, or choose no target.",
        "Korpus n'estime pas d'objectif calorique pour les moins de 18 ans. Demandez conseil à un médecin ou à un diététicien, ou choisissez « Aucun objectif ».",
        "Korpus no estima objetivos calóricos para menores de 18 años. Consulte a un médico o a un dietista-nutricionista, o elija «Sin objetivo».",
    ),
    (
        "To estimate a target, fill in sex, date of birth, height, weight and activity level.",
        "Pour estimer un objectif, renseignez le sexe, la date de naissance, la taille, le poids et le niveau d'activité.",
        "Para estimar un objetivo, indique sexo, fecha de nacimiento, altura, peso y nivel de actividad.",
    ),
    (
        "Enter your daily target in kcal.",
        "Saisissez votre objectif quotidien en kcal.",
        "Introduzca su objetivo diario en kcal.",
    ),
    ("Target", "Objectif", "Objetivo"),
    (
        "Choose how to set your target.",
        "Choisissez comment définir votre objectif.",
        "Elija cómo establecer su objetivo.",
    ),
    (
        "This target is below 1200 kcal. Very low intakes are not suitable for most adults without medical supervision.",
        "Cet objectif est inférieur à 1200 kcal. Des apports très faibles ne conviennent pas à la plupart des adultes sans suivi médical.",
        "Este objetivo está por debajo de 1200 kcal. Una ingesta muy baja no es adecuada para la mayoría de los adultos sin supervisión médica.",
    ),
    ("Weight", "Poids", "Peso"),
    ("Height", "Taille", "Altura"),
    (
        "Enter your weight.",
        "Saisissez votre poids.",
        "Introduzca su peso.",
    ),
    ("Note", "Note", "Nota"),
    ("Edit weight", "Modifier le poids", "Editar peso"),
    (
        "There is already a weight for this date. Edit that entry instead.",
        "Un poids existe déjà pour cette date. Modifiez plutôt cette entrée.",
        "Ya hay un peso para esta fecha. Edite esa entrada.",
    ),
    (
        "Enter a weight between {0} and {1} kg (or the equivalent in lb).",
        "Saisissez un poids entre {0} et {1} kg (ou l'équivalent en lb).",
        "Introduzca un peso entre {0} y {1} kg (o el equivalente en lb).",
    ),
    (
        "Weight from {0} to {1}: {2} entries, from {3} to {4} ({5}).",
        "Poids du {0} au {1} : {2} entrées, de {3} à {4} ({5}).",
        "Peso del {0} al {1}: {2} entradas, de {3} a {4} ({5}).",
    ),
    (
        "Cross-site form submissions are not allowed.",
        "Les envois de formulaire depuis un autre site ne sont pas autorisés.",
        "No se permiten envíos de formularios desde otro sitio.",
    ),
    (
        "Cross-origin form submissions are not allowed.",
        "Les envois de formulaire depuis une autre origine ne sont pas autorisés.",
        "No se permiten envíos de formularios desde otro origen.",
    ),
    (
        "Added to your diary.",
        "Ajouté à votre journal.",
        "Añadido a su diario.",
    ),
    (
        "Entry updated.",
        "Entrée mise à jour.",
        "Entrada actualizada.",
    ),
    ("Entry deleted.", "Entrée supprimée.", "Entrada eliminada."),
    ("Food created.", "Aliment créé.", "Alimento creado."),
    (
        "Food saved. Existing diary entries keep their logged values.",
        "Aliment enregistré. Les entrées existantes du journal conservent leurs valeurs.",
        "Alimento guardado. Las entradas existentes del diario conservan sus valores.",
    ),
    (
        "Food deleted. Existing diary entries keep their logged values.",
        "Aliment supprimé. Les entrées existantes du journal conservent leurs valeurs.",
        "Alimento eliminado. Las entradas existentes del diario conservan sus valores.",
    ),
    ("Weight recorded.", "Poids enregistré.", "Peso registrado."),
    (
        "Weight for that date updated.",
        "Poids de cette date mis à jour.",
        "Peso de esa fecha actualizado.",
    ),
    (
        "Weight entry deleted.",
        "Entrée de poids supprimée.",
        "Entrada de peso eliminada.",
    ),
    (
        "Goal settings saved.",
        "Objectifs enregistrés.",
        "Objetivos guardados.",
    ),
    (
        "Settings saved.",
        "Paramètres enregistrés.",
        "Ajustes guardados.",
    ),
    (
        "Import complete.",
        "Import terminé.",
        "Importación completada.",
    ),
    ("Foods", "Aliments", "Alimentos"),
    (
        "Energy was provided in kJ and converted to kcal (1 kcal = 4.184 kJ).",
        "L'énergie était indiquée en kJ et a été convertie en kcal (1 kcal = 4,184 kJ).",
        "La energía venía en kJ y se convirtió a kcal (1 kcal = 4,184 kJ).",
    ),
    (
        "the service did not respond in time",
        "le service n'a pas répondu à temps",
        "el servicio no respondió a tiempo",
    ),
    (
        "too many searches in a short time; please wait a minute",
        "trop de recherches en peu de temps ; patientez une minute",
        "demasiadas búsquedas en poco tiempo; espere un minuto",
    ),
    (
        "the service is currently unavailable",
        "le service est actuellement indisponible",
        "el servicio no está disponible en este momento",
    ),
    (
        "the service returned unexpected data",
        "le service a renvoyé des données inattendues",
        "el servicio devolvió datos inesperados",
    ),
    ("Record weight", "Enregistrer un poids", "Registrar peso"),
    ("Date", "Date", "Fecha"),
    ("(optional)", "(facultatif)", "(opcional)"),
    (
        "One entry per day; recording again for the same date replaces it. Your calorie target does not change automatically.",
        "Une entrée par jour ; un nouvel enregistrement pour la même date la remplace. Votre objectif calorique ne change pas automatiquement.",
        "Una entrada por día; registrar de nuevo la misma fecha la sustituye. Su objetivo calórico no cambia automáticamente.",
    ),
    ("Save weight", "Enregistrer le poids", "Guardar peso"),
    ("Latest", "Dernière mesure", "Última medición"),
    (
        "The list below shows every entry.",
        "La liste ci-dessous montre toutes les entrées.",
        "La lista de abajo muestra todas las entradas.",
    ),
    ("History", "Historique", "Historial"),
    (
        "No weights recorded yet.",
        "Aucun poids enregistré pour l'instant.",
        "Todavía no hay pesos registrados.",
    ),
    ("Change", "Variation", "Cambio"),
    ("Edit", "Modifier", "Editar"),
    (
        "Weight trend in {0}",
        "Évolution du poids en {0}",
        "Evolución del peso en {0}",
    ),
    ("Previous day", "Jour précédent", "Día anterior"),
    ("Previous", "Précédent", "Anterior"),
    ("Diary date", "Date du journal", "Fecha del diario"),
    ("Go", "Aller", "Ir"),
    ("Next day", "Jour suivant", "Día siguiente"),
    ("Next", "Suivant", "Siguiente"),
    ("Back to today", "Revenir à aujourd'hui", "Volver a hoy"),
    ("Daily summary", "Résumé du jour", "Resumen del día"),
    ("remaining", "restantes", "restantes"),
    (
        "above target",
        "au-dessus de l'objectif",
        "por encima del objetivo",
    ),
    ("kcal eaten", "kcal consommées", "kcal consumidas"),
    (
        "No daily target set.",
        "Aucun objectif quotidien défini.",
        "No hay objetivo diario.",
    ),
    (
        "Set a goal",
        "Définir un objectif",
        "Establecer un objetivo",
    ),
    (
        "Some entries have missing nutrition values, so totals may be higher.",
        "Certaines entrées n'ont pas toutes leurs valeurs nutritionnelles ; les totaux peuvent donc être plus élevés.",
        "Algunas entradas no tienen todos los valores nutricionales, así que los totales pueden ser mayores.",
    ),
    ("Add food", "Ajouter un aliment", "Añadir alimento"),
    ("incomplete", "incomplet", "incompleto"),
    ("Nothing logged.", "Rien d'enregistré.", "Nada registrado."),
    ("P", "P", "P"),
    ("C", "G", "C"),
    ("F", "L", "G"),
    ("values missing", "valeurs manquantes", "faltan valores"),
    ("of {0} kcal", "sur {0} kcal", "de {0} kcal"),
    (
        "Delete {0} from {1}?",
        "Supprimer {0} de {1} ?",
        "¿Eliminar {0} de {1}?",
    ),
    (
        "External results could not be loaded. Your own foods are still available above.",
        "Les résultats externes n'ont pas pu être chargés. Vos propres aliments restent disponibles ci-dessus.",
        "No se pudieron cargar los resultados externos. Sus propios alimentos siguen disponibles arriba.",
    ),
    ("This amount:", "Cette quantité :", "Esta cantidad:"),
    ("Skip to content", "Aller au contenu", "Saltar al contenido"),
    ("Main", "Principal", "Principal"),
    ("Diary", "Journal", "Diario"),
    (
        "Korpus keeps your data on this server. Calorie estimates are approximate and not medical advice.",
        "Korpus conserve vos données sur ce serveur. Les estimations caloriques sont approximatives et ne constituent pas un avis médical.",
        "Korpus guarda sus datos en este servidor. Las estimaciones de calorías son aproximadas y no son consejo médico.",
    ),
    (
        "This food is used in 1 diary entry.",
        "Cet aliment est utilisé dans 1 entrée du journal.",
        "Este alimento se usa en 1 entrada del diario.",
    ),
    (
        "This food is used in {0} diary entries.",
        "Cet aliment est utilisé dans {0} entrées du journal.",
        "Este alimento se usa en {0} entradas del diario.",
    ),
    (
        "Those entries will stay in your diary with the values they were logged with.",
        "Ces entrées resteront dans votre journal avec les valeurs enregistrées.",
        "Esas entradas permanecerán en su diario con los valores con que se registraron.",
    ),
    (
        "This food is not used in your diary.",
        "Cet aliment n'est pas utilisé dans votre journal.",
        "Este alimento no se usa en su diario.",
    ),
    (
        "This cannot be undone.",
        "Cette action est irréversible.",
        "Esta acción no se puede deshacer.",
    ),
    ("Delete food", "Supprimer l'aliment", "Eliminar alimento"),
    ("Cancel", "Annuler", "Cancelar"),
    ("Delete “{0}”?", "Supprimer « {0} » ?", "¿Eliminar «{0}»?"),
    (
        "Go to today's diary",
        "Aller au journal du jour",
        "Ir al diario de hoy",
    ),
    ("Back to diary", "Retour au journal", "Volver al diario"),
    ("Edit entry", "Modifier l'entrée", "Editar entrada"),
    ("View food", "Voir l'aliment", "Ver alimento"),
    ("Meal", "Repas", "Comida"),
    ("Save", "Enregistrer", "Guardar"),
    (
        "Delete this entry?",
        "Supprimer cette entrée ?",
        "¿Eliminar esta entrada?",
    ),
    ("Delete entry", "Supprimer l'entrée", "Eliminar entrada"),
    (
        "logged values per 100 {0}:",
        "valeurs enregistrées pour 100 {0} :",
        "valores registrados por 100 {0}:",
    ),
    ("General", "Général", "General"),
    (
        "Save settings",
        "Enregistrer les paramètres",
        "Guardar ajustes",
    ),
    ("Your data", "Vos données", "Sus datos"),
    (
        "Everything stays on this server. Download a copy regularly.",
        "Tout reste sur ce serveur. Téléchargez-en régulièrement une copie.",
        "Todo se queda en este servidor. Descargue una copia con regularidad.",
    ),
    (
        "Database backup (.db)",
        "Sauvegarde de la base (.db)",
        "Copia de la base de datos (.db)",
    ),
    (
        "Full export (JSON)",
        "Export complet (JSON)",
        "Exportación completa (JSON)",
    ),
    ("Diary (CSV)", "Journal (CSV)", "Diario (CSV)"),
    (
        "The database backup is a consistent snapshot that can replace the database file to restore. The JSON export can be imported into a new, empty Korpus installation.",
        "La sauvegarde de la base est un instantané cohérent qui peut remplacer le fichier de base de données pour une restauration. L'export JSON peut être importé dans une nouvelle installation Korpus vide.",
        "La copia de la base de datos es una instantánea coherente que puede sustituir al archivo de la base de datos para restaurar. La exportación JSON se puede importar en una instalación nueva y vacía de Korpus.",
    ),
    (
        "Import a JSON export",
        "Importer un export JSON",
        "Importar una exportación JSON",
    ),
    ("Export file", "Fichier d'export", "Archivo de exportación"),
    (
        "Only possible while this installation has no foods, diary entries, weights or activity.",
        "Possible uniquement tant que cette installation ne contient ni aliments, ni entrées de journal, ni poids, ni activité.",
        "Solo es posible mientras esta instalación no tenga alimentos, entradas del diario, pesos ni actividad.",
    ),
    ("Import", "Importer", "Importar"),
    (
        "Food databases",
        "Bases d'aliments",
        "Bases de datos de alimentos",
    ),
    (
        "No external food databases are enabled. Search uses only the foods saved on this server.",
        "Aucune base d'aliments externe n'est activée. La recherche n'utilise que les aliments enregistrés sur ce serveur.",
        "No hay bases de datos de alimentos externas activadas. La búsqueda solo usa los alimentos guardados en este servidor.",
    ),
    (
        "When you search, your search words and interface language are sent to these services once you pause typing. Nothing else — no diary, weight or profile data — is ever sent.",
        "Lorsque vous recherchez, vos termes de recherche et la langue de l'interface sont envoyés à ces services dès que vous marquez une pause dans la saisie. Rien d'autre — ni journal, ni poids, ni profil — n'est jamais envoyé.",
        "Cuando busca, sus términos de búsqueda y el idioma de la interfaz se envían a estos servicios en cuanto deja de escribir un momento. Nunca se envía nada más: ni diario, ni peso, ni datos del perfil.",
    ),
    (
        "The server administrator can turn providers on or off with environment variables, for example:",
        "L'administrateur du serveur peut activer ou désactiver les sources avec des variables d'environnement, par exemple :",
        "El administrador del servidor puede activar o desactivar las fuentes con variables de entorno, por ejemplo:",
    ),
    ("Back to weight", "Retour au poids", "Volver al peso"),
    (
        "Delete this weight entry?",
        "Supprimer cette entrée de poids ?",
        "¿Eliminar esta entrada de peso?",
    ),
    (
        "Open the food that already uses this barcode",
        "Ouvrir l'aliment qui utilise déjà ce code-barres",
        "Abrir el alimento que ya usa este código de barras",
    ),
    ("Product", "Produit", "Producto"),
    ("(required)", "(obligatoire)", "(obligatorio)"),
    ("Barcode", "Code-barres", "Código de barras"),
    (
        "The digits under the barcode, so you can find the product by scanning or typing it later.",
        "Les chiffres sous le code-barres, pour retrouver le produit plus tard en le scannant ou en le saisissant.",
        "Los dígitos bajo el código de barras, para encontrar el producto más tarde escaneándolo o escribiéndolo.",
    ),
    (
        "Values are given",
        "Les valeurs sont indiquées",
        "Los valores se indican",
    ),
    (
        "per 100 g (solid foods)",
        "pour 100 g (aliments solides)",
        "por 100 g (alimentos sólidos)",
    ),
    (
        "per 100 ml (drinks, liquids)",
        "pour 100 ml (boissons, liquides)",
        "por 100 ml (bebidas, líquidos)",
    ),
    (
        "Nutrition per 100 g or 100 ml",
        "Valeurs nutritionnelles pour 100 g ou 100 ml",
        "Información nutricional por 100 g o 100 ml",
    ),
    (
        "Copy the values from the label. Decimal commas are fine. Use 0 for none.",
        "Recopiez les valeurs de l'étiquette. Les virgules décimales sont acceptées. Saisissez 0 si la valeur est nulle.",
        "Copie los valores de la etiqueta. Se admiten comas decimales. Use 0 si no hay.",
    ),
    ("Energy unit", "Unité d'énergie", "Unidad de energía"),
    ("Serving", "Portion", "Ración"),
    (
        "Serving size in g or ml",
        "Taille de portion en g ou ml",
        "Tamaño de la ración en g o ml",
    ),
    ("e.g. 1 slice", "p. ex. 1 tranche", "p. ej. 1 rebanada"),
    (
        "Save changes",
        "Enregistrer les modifications",
        "Guardar cambios",
    ),
    ("Create food", "Créer l'aliment", "Crear alimento"),
    (
        "Diary entries you already logged keep their original values.",
        "Les entrées déjà enregistrées dans le journal conservent leurs valeurs d'origine.",
        "Las entradas ya registradas en el diario conservan sus valores originales.",
    ),
    (
        "Please correct the following",
        "Veuillez corriger les points suivants",
        "Corrija lo siguiente",
    ),
    (
        "Search foods by name, brand or barcode",
        "Rechercher des aliments par nom, marque ou code-barres",
        "Buscar alimentos por nombre, marca o código de barras",
    ),
    ("Search", "Rechercher", "Buscar"),
    ("Clear", "Effacer", "Borrar"),
    (
        "Clear history?",
        "Effacer l'historique ?",
        "¿Borrar el historial?",
    ),
    (
        "Your diary and saved foods are kept.",
        "Votre journal et vos aliments enregistrés sont conservés.",
        "Se conservan su diario y sus alimentos guardados.",
    ),
    ("Delete", "Supprimer", "Eliminar"),
    ("Remove", "Retirer", "Quitar"),
    (
        "Clear history",
        "Effacer l'historique",
        "Borrar el historial",
    ),
    (
        "Foods you add to your diary will appear here.",
        "Les aliments que vous ajoutez au journal apparaîtront ici.",
        "Los alimentos que añada al diario aparecerán aquí.",
    ),
    (
        "History cleared. Your diary and saved foods are unchanged.",
        "Historique effacé. Votre journal et vos aliments enregistrés sont inchangés.",
        "Historial borrado. Su diario y sus alimentos guardados no han cambiado.",
    ),
    ("Can't find it?", "Introuvable ?", "¿No lo encuentra?"),
    (
        "Create the food yourself using the values from its nutrition label.",
        "Créez l'aliment vous-même à partir des valeurs de son étiquette nutritionnelle.",
        "Cree el alimento usted mismo con los valores de su etiqueta nutricional.",
    ),
    (
        "Logging for {0}.",
        "Enregistrement pour le {0}.",
        "Registro para el {0}.",
    ),
    (
        "Logging for {0} · {1}.",
        "Enregistrement pour le {0} · {1}.",
        "Registro para el {0} · {1}.",
    ),
    (
        "Optionally set a daily calorie target now. You can change or remove it any time.",
        "Vous pouvez dès maintenant définir un objectif calorique quotidien. Vous pourrez le modifier ou le supprimer à tout moment.",
        "Si quiere, establezca ahora un objetivo calórico diario. Puede cambiarlo o quitarlo cuando quiera.",
    ),
    (
        "Skip — go to today's diary",
        "Passer — aller au journal du jour",
        "Omitir — ir al diario de hoy",
    ),
    (
        "Current daily target",
        "Objectif quotidien actuel",
        "Objetivo diario actual",
    ),
    (
        "No target. Your diary shows what you eat without a target.",
        "Aucun objectif. Votre journal affiche ce que vous mangez sans objectif.",
        "Sin objetivo. Su diario muestra lo que come sin un objetivo.",
    ),
    ("About you", "À propos de vous", "Sobre usted"),
    (
        "Only needed to estimate a target. These details stay on this server and are used for the Mifflin–St Jeor estimate of resting energy needs, multiplied by your activity level.",
        "Nécessaire uniquement pour estimer un objectif. Ces informations restent sur ce serveur et servent à l'estimation de Mifflin–St Jeor des besoins énergétiques au repos, multipliée par votre niveau d'activité.",
        "Solo se necesita para estimar un objetivo. Estos datos se quedan en este servidor y se usan para la estimación de Mifflin–St Jeor de las necesidades energéticas en reposo, multiplicada por su nivel de actividad.",
    ),
    (
        "Sex used by the equation",
        "Sexe utilisé par l'équation",
        "Sexo usado por la ecuación",
    ),
    ("Female", "Femme", "Mujer"),
    ("Male", "Homme", "Hombre"),
    (
        "Prefer not to say",
        "Je préfère ne pas répondre",
        "Prefiero no decirlo",
    ),
    (
        "The equation has separate formulas; without this, Korpus cannot estimate.",
        "L'équation a des formules distinctes ; sans cette information, Korpus ne peut pas estimer.",
        "La ecuación tiene fórmulas distintas; sin este dato, Korpus no puede estimar.",
    ),
    ("Date of birth", "Date de naissance", "Fecha de nacimiento"),
    (
        "Energy needs change with age.",
        "Les besoins énergétiques évoluent avec l'âge.",
        "Las necesidades energéticas cambian con la edad.",
    ),
    (
        "Prefilled from your latest weigh-in. The target only changes when you save this form.",
        "Prérempli avec votre dernière pesée. L'objectif ne change que lorsque vous enregistrez ce formulaire.",
        "Rellenado con su último pesaje. El objetivo solo cambia al guardar este formulario.",
    ),
    ("Activity level", "Niveau d'activité", "Nivel de actividad"),
    ("Not set", "Non défini", "Sin definir"),
    ("Goal", "Objectif", "Meta"),
    (
        "I am pregnant or breastfeeding, or have a medical condition that affects my nutrition",
        "Je suis enceinte ou j'allaite, ou j'ai un problème de santé qui influe sur mon alimentation",
        "Estoy embarazada o en período de lactancia, o tengo una afección médica que influye en mi alimentación",
    ),
    (
        "If checked, Korpus will not suggest a weight-loss target.",
        "Si cette case est cochée, Korpus ne proposera pas d'objectif de perte de poids.",
        "Si se marca, Korpus no sugerirá un objetivo de pérdida de peso.",
    ),
    (
        "Korpus does not estimate calorie targets for people under 18. A doctor or registered dietitian can give advice that fits you.",
        "Korpus n'estime pas d'objectif calorique pour les moins de 18 ans. Un médecin ou un diététicien peut vous donner des conseils adaptés.",
        "Korpus no estima objetivos calóricos para menores de 18 años. Un médico o un dietista-nutricionista puede darle consejos adecuados.",
    ),
    (
        "Estimate from these details",
        "Estimation à partir de ces informations",
        "Estimación a partir de estos datos",
    ),
    (
        "Resting energy (Mifflin–St Jeor)",
        "Énergie au repos (Mifflin–St Jeor)",
        "Energía en reposo (Mifflin–St Jeor)",
    ),
    ("kcal/day", "kcal/jour", "kcal/día"),
    (
        "Estimated maintenance",
        "Maintien estimé",
        "Mantenimiento estimado",
    ),
    (
        "Estimates are approximate and are not medical advice. Individual needs vary; for personal guidance, speak to a qualified health professional.",
        "Les estimations sont approximatives et ne constituent pas un avis médical. Les besoins varient d'une personne à l'autre ; pour des conseils personnalisés, adressez-vous à un professionnel de santé qualifié.",
        "Las estimaciones son aproximadas y no son consejo médico. Las necesidades varían según la persona; para una orientación personal, consulte a un profesional sanitario cualificado.",
    ),
    ("Daily target", "Objectif quotidien", "Objetivo diario"),
    (
        "How should your target be set?",
        "Comment définir votre objectif ?",
        "¿Cómo quiere establecer su objetivo?",
    ),
    (
        "Use the estimate from the details above",
        "Utiliser l'estimation à partir des informations ci-dessus",
        "Usar la estimación de los datos anteriores",
    ),
    (
        "Set my own target",
        "Définir mon propre objectif",
        "Establecer mi propio objetivo",
    ),
    ("No target", "Aucun objectif", "Sin objetivo"),
    (
        "Own target (kcal per day)",
        "Objectif personnel (kcal par jour)",
        "Objetivo propio (kcal por día)",
    ),
    (
        "Macronutrient goals",
        "Objectifs de macronutriments",
        "Objetivos de macronutrientes",
    ),
    (
        "Save goals",
        "Enregistrer les objectifs",
        "Guardar objetivos",
    ),
    (
        "Suggested target ({0})",
        "Objectif suggéré ({0})",
        "Objetivo sugerido ({0})",
    ),
    ("{0} (g per day)", "{0} (g par jour)", "{0} (g por día)"),
    (
        "How should your macronutrient goals be set?",
        "Comment définir vos objectifs de macronutriments ?",
        "¿Cómo quiere establecer sus objetivos de macronutrientes?",
    ),
    (
        "Estimate from my daily target, goal and weight",
        "Estimer à partir de mon objectif quotidien, de mon but et de mon poids",
        "Estimar a partir de mi objetivo diario, mi meta y mi peso",
    ),
    (
        "Set my own goals",
        "Définir mes propres objectifs",
        "Establecer mis propios objetivos",
    ),
    (
        "Estimate for a day without recorded activity",
        "Estimation pour un jour sans activité enregistrée",
        "Estimación para un día sin actividad registrada",
    ),
    (
        "{0} g per kg of body weight",
        "{0} g par kg de poids corporel",
        "{0} g por kg de peso corporal",
    ),
    (
        "{0} g per lb of body weight",
        "{0} g par lb de poids corporel",
        "{0} g por lb de peso corporal",
    ),
    ("30% of energy", "30 % de l'énergie", "30 % de la energía"),
    (
        "the rest of the energy",
        "le reste de l'énergie",
        "el resto de la energía",
    ),
    (
        "Protein was limited to 35% of your energy target, so there is room for carbohydrate.",
        "Les protéines ont été limitées à 35 % de votre objectif énergétique, pour laisser de la place aux glucides.",
        "Las proteínas se limitaron al 35 % de su objetivo energético, para dejar espacio a los carbohidratos.",
    ),
    (
        "Steps and sport recorded for a day add their energy to that day's carbohydrate goal, the main fuel of exercise.",
        "Les pas et le sport enregistrés pour un jour ajoutent leur énergie à l'objectif de glucides de ce jour, principal carburant de l'effort.",
        "Los pasos y el deporte registrados en un día suman su energía al objetivo de carbohidratos de ese día, el principal combustible del ejercicio.",
    ),
    (
        "To estimate macronutrient goals, set a daily calorie target.",
        "Pour estimer les objectifs de macronutriments, définissez un objectif calorique quotidien.",
        "Para estimar los objetivos de macronutrientes, establezca un objetivo calórico diario.",
    ),
    (
        "To estimate macronutrient goals, enter your weight.",
        "Pour estimer les objectifs de macronutriments, indiquez votre poids.",
        "Para estimar los objetivos de macronutrientes, indique su peso.",
    ),
    (
        "Korpus does not estimate macronutrient goals for people under 18. Please ask a doctor or registered dietitian, or enter your own.",
        "Korpus n'estime pas d'objectifs de macronutriments pour les moins de 18 ans. Demandez conseil à un médecin ou à un diététicien, ou saisissez les vôtres.",
        "Korpus no estima objetivos de macronutrientes para menores de 18 años. Consulte a un médico o a un dietista-nutricionista, o introduzca los suyos.",
    ),
    (
        "Missing values:",
        "Valeurs manquantes :",
        "Valores que faltan:",
    ),
    (
        "Totals will not include them.",
        "Les totaux ne les incluront pas.",
        "Los totales no los incluirán.",
    ),
    (
        "Add them if you know them.",
        "Ajoutez-les si vous les connaissez.",
        "Añádalos si los conoce.",
    ),
    ("Add to diary", "Ajouter au journal", "Añadir al diario"),
    (
        "Add to which meal?",
        "Ajouter à quel repas ?",
        "¿Añadir a qué comida?",
    ),
    ("Quick amounts", "Quantités rapides", "Cantidades rápidas"),
    (
        "Nutrition",
        "Valeurs nutritionnelles",
        "Información nutricional",
    ),
    ("Nutrient", "Nutriment", "Nutriente"),
    (
        "Energy was given in kJ and converted (1 kcal = 4.184 kJ).",
        "L'énergie était indiquée en kJ et a été convertie (1 kcal = 4,184 kJ).",
        "La energía venía en kJ y se convirtió (1 kcal = 4,184 kJ).",
    ),
    ("Source", "Source", "Fuente"),
    ("Data source", "Source des données", "Fuente de los datos"),
    (
        "(edited by you)",
        "(modifié par vous)",
        "(editado por usted)",
    ),
    (
        "Last updated at source",
        "Dernière mise à jour à la source",
        "Última actualización en la fuente",
    ),
    ("Retrieved", "Récupéré le", "Obtenido el"),
    (
        "Saved in Korpus",
        "Enregistré dans Korpus",
        "Guardado en Korpus",
    ),
    (
        "View on Open Food Facts",
        "Voir sur Open Food Facts",
        "Ver en Open Food Facts",
    ),
    ("Edit food", "Modifier l'aliment", "Editar alimento"),
    (
        "Nutrition values for {0}",
        "Valeurs nutritionnelles de {0}",
        "Valores nutricionales de {0}",
    ),
    (
        "1 serving ({0} {1})",
        "1 portion ({0} {1})",
        "1 ración ({0} {1})",
    ),
    ("Language", "Langue", "Idioma"),
    (
        "Also the language in which food databases are searched and product names are shown.",
        "C'est aussi la langue dans laquelle les bases d'aliments sont interrogées et les noms de produits affichés.",
        "También es el idioma en el que se consultan las bases de datos de alimentos y se muestran los nombres de los productos.",
    ),
    (
        "Units for body weight and height",
        "Unités pour le poids et la taille",
        "Unidades para el peso y la altura",
    ),
    ("Metric (kg, cm)", "Métriques (kg, cm)", "Métricas (kg, cm)"),
    (
        "Imperial (lb, in)",
        "Impériales (lb, in)",
        "Imperiales (lb, in)",
    ),
    (
        "Food amounts always use grams or millilitres, as on nutrition labels.",
        "Les quantités d'aliments sont toujours en grammes ou en millilitres, comme sur les étiquettes nutritionnelles.",
        "Las cantidades de alimentos siempre se indican en gramos o mililitros, como en las etiquetas nutricionales.",
    ),
    ("Time zone", "Fuseau horaire", "Zona horaria"),
    (
        "Decides when a new diary day starts, e.g. Europe/Berlin or America/Chicago.",
        "Détermine quand commence un nouveau jour du journal, p. ex. Europe/Paris ou America/Montreal.",
        "Determina cuándo empieza un nuevo día del diario, p. ej. Europe/Madrid o America/Mexico_City.",
    ),
    (
        "Korpus is a private food diary that runs on your own server. A few quick settings, then you can start logging.",
        "Korpus est un journal alimentaire privé qui fonctionne sur votre propre serveur. Quelques réglages rapides, et vous pourrez commencer.",
        "Korpus es un diario de alimentación privado que funciona en su propio servidor. Unos ajustes rápidos y podrá empezar a registrar.",
    ),
    ("Continue", "Continuer", "Continuar"),
    (
        "Your own foods still work.",
        "Vos propres aliments restent disponibles.",
        "Sus propios alimentos siguen disponibles.",
    ),
    ("select", "sélectionner", "seleccionar"),
    (
        "{0} could not be searched: {1}.",
        "Impossible d'interroger {0} : {1}.",
        "No se pudo buscar en {0}: {1}.",
    ),
    ("Missing: {0}", "Manquant : {0}", "Falta: {0}"),
    ("Results", "Résultats", "Resultados"),
    (
        "Many searches in a short time; more results will appear in a moment…",
        "Beaucoup de recherches en peu de temps ; d'autres résultats vont apparaître dans un instant…",
        "Muchas búsquedas en poco tiempo; en un momento aparecerán más resultados…",
    ),
    (
        "Searching food databases…",
        "Recherche dans les bases d'aliments…",
        "Buscando en las bases de datos de alimentos…",
    ),
    ("No results.", "Aucun résultat.", "Sin resultados."),
    (
        "Result pages",
        "Pages de résultats",
        "Páginas de resultados",
    ),
    (
        "Where the energy comes from",
        "Origine des calories",
        "Origen de las calorías",
    ),
    ("Appearance", "Apparence", "Apariencia"),
    ("Automatic", "Automatique", "Automático"),
    ("Light", "Clair", "Claro"),
    ("Dark", "Sombre", "Oscuro"),
    (
        "Automatic follows your device. Dark uses true black, which saves power on OLED screens. Saved on this device only.",
        "Automatique suit votre appareil. Sombre utilise un noir pur, qui économise l'énergie sur les écrans OLED. Enregistré sur cet appareil uniquement.",
        "Automático sigue a su dispositivo. Oscuro usa negro puro, que ahorra energía en pantallas OLED. Se guarda solo en este dispositivo.",
    ),
    ("Choose file", "Choisir un fichier", "Elegir archivo"),
    (
        "No file chosen",
        "Aucun fichier choisi",
        "Ningún archivo elegido",
    ),
    ("{0} files", "{0} fichiers", "{0} archivos"),
    (
        "Could not save. Check your connection and try again.",
        "Enregistrement impossible. Vérifiez votre connexion et réessayez.",
        "No se pudo guardar. Compruebe su conexión e inténtelo de nuevo.",
    ),
    (
        "Save appearance",
        "Enregistrer l'apparence",
        "Guardar apariencia",
    ),
    (
        "Appearance saved on this device.",
        "Apparence enregistrée sur cet appareil.",
        "Apariencia guardada en este dispositivo.",
    ),
    (
        "Choose an appearance.",
        "Choisissez une apparence.",
        "Elija una apariencia.",
    ),
    (
        "Tables are listed in the order that best fits {0}; search uses the first one imported.",
        "Les tables sont classées dans l'ordre le mieux adapté à : {0} ; la recherche utilise la première importée.",
        "Las tablas aparecen en el orden que mejor se adapta a {0}; la búsqueda usa la primera importada.",
    ),
    (
        "Download the Excel file “Nutrient profiles”.",
        "Téléchargez le fichier Excel « Nutrient profiles ».",
        "Descargue el archivo Excel «Nutrient profiles».",
    ),
    (
        "Official download page",
        "Page de téléchargement officielle",
        "Página de descarga oficial",
    ),
    ("Not imported", "Non importée", "No importada"),
    (
        "Food table removed. Foods you already saved from it are kept.",
        "Table supprimée. Les aliments que vous en aviez déjà enregistrés sont conservés.",
        "Tabla eliminada. Los alimentos que ya guardó de ella se conservan.",
    ),
    ("Remove table", "Supprimer la table", "Eliminar la tabla"),
    (
        "Search includes generic foods from {0}.",
        "La recherche inclut les aliments génériques de {0}.",
        "La búsqueda incluye los alimentos genéricos de {0}.",
    ),
    (
        "Import and replace",
        "Importer et remplacer",
        "Importar y reemplazar",
    ),
    (
        "Download the CSV zip files for Foundation Foods and SR Legacy (and optionally FNDDS survey foods). You can upload several at once. Branded foods are not supported.",
        "Téléchargez les fichiers zip CSV « Foundation Foods » et « SR Legacy » (et, si vous le souhaitez, « FNDDS »). Vous pouvez en envoyer plusieurs à la fois. Les produits de marque (« Branded Foods ») ne sont pas pris en charge.",
        "Descargue los archivos zip CSV de «Foundation Foods» y «SR Legacy» (y, si lo desea, «FNDDS»). Puede subir varios a la vez. Los productos de marca («Branded Foods») no son compatibles.",
    ),
    (
        "Canadian Nutrient File, Health Canada. Open Government Licence – Canada.",
        "Fichier canadien sur les éléments nutritifs, Santé Canada. Licence du gouvernement ouvert – Canada.",
        "Canadian Nutrient File, Health Canada. Licencia de Gobierno Abierto – Canadá.",
    ),
    (
        "Downloaded files",
        "Fichiers téléchargés",
        "Archivos descargados",
    ),
    (
        "Food composition tables",
        "Tables de composition des aliments",
        "Tablas de composición de alimentos",
    ),
    (
        "{0} foods, imported {1}",
        "{0} aliments, importée le {1}",
        "{0} alimentos, importada el {1}",
    ),
    (
        "Food table imported. Search now uses it where it fits your country.",
        "Table importée. La recherche l'utilise désormais si elle convient à votre pays.",
        "Tabla importada. La búsqueda la usará cuando corresponda a su país.",
    ),
    (
        "Australian Food Composition Database, © Food Standards Australia New Zealand, used under its data user licence. FSANZ does not endorse Korpus.",
        "Australian Food Composition Database, © Food Standards Australia New Zealand, utilisée selon sa licence d'utilisation des données. FSANZ ne cautionne pas Korpus.",
        "Australian Food Composition Database, © Food Standards Australia New Zealand, usada según su licencia de uso de datos. FSANZ no respalda a Korpus.",
    ),
    (
        "Download the CSV zip file of the Canadian Nutrient File.",
        "Téléchargez le fichier zip CSV du Fichier canadien sur les éléments nutritifs.",
        "Descargue el archivo zip CSV del Canadian Nutrient File.",
    ),
    ("unknown country", "pays inconnu", "país desconocido"),
    (
        "Update or remove",
        "Mettre à jour ou supprimer",
        "Actualizar o eliminar",
    ),
    (
        "Used for search",
        "Utilisée pour la recherche",
        "Usada en la búsqueda",
    ),
    (
        "No foods were found in the upload for {0}.",
        "Aucun aliment n'a été trouvé dans le fichier envoyé pour {0}.",
        "No se encontraron alimentos en el archivo subido para {0}.",
    ),
    (
        "Chooses the national food composition table that search uses. It is never sent anywhere.",
        "Détermine la table nationale de composition des aliments utilisée par la recherche. Cette information n'est jamais transmise.",
        "Determina la tabla nacional de composición de alimentos que usa la búsqueda. Nunca se envía a ningún sitio.",
    ),
    ("Country", "Pays", "País"),
    (
        "No table is imported yet. Import one to find generic foods such as fruit, vegetables, meat or cooked dishes.",
        "Aucune table n'est encore importée. Importez-en une pour trouver des aliments génériques : fruits, légumes, viandes ou plats cuisinés.",
        "Todavía no hay ninguna tabla importada. Importe una para encontrar alimentos genéricos como frutas, verduras, carnes o platos cocinados.",
    ),
    (
        "Download the Excel file of the Composition of Foods Integrated Dataset.",
        "Téléchargez le fichier Excel « Composition of Foods Integrated Dataset ».",
        "Descargue el archivo Excel «Composition of Foods Integrated Dataset».",
    ),
    (
        "Salt was calculated from sodium (salt = sodium × 2.5).",
        "Le sel a été calculé à partir du sodium (sel = sodium × 2,5).",
        "La sal se calculó a partir del sodio (sal = sodio × 2,5).",
    ),
    (
        "Downloaded file",
        "Fichier téléchargé",
        "Archivo descargado",
    ),
    (
        "Download the XML files alim_….xml and compo_….xml and upload both at once (or the XML zip file of the 2020 release).",
        "Téléchargez les fichiers XML alim_….xml et compo_….xml et envoyez-les ensemble (ou le fichier zip XML de la version 2020).",
        "Descargue los archivos XML alim_….xml y compo_….xml y súbalos juntos (o el archivo zip XML de la versión 2020).",
    ),
    (
        "{0} foods, {1} included with Korpus, installed {2}",
        "{0} aliments, {1} incluse avec Korpus, installée le {2}",
        "{0} alimentos, {1} incluida con Korpus, instalada el {2}",
    ),
    (
        "Use {0}, included with Korpus",
        "Utiliser {0}, incluse avec Korpus",
        "Usar {0}, incluida con Korpus",
    ),
    (
        "No download needed. Or import the publisher's files below, for example a newer release.",
        "Aucun téléchargement nécessaire. Vous pouvez aussi importer ci-dessous les fichiers de l'éditeur, par exemple une version plus récente.",
        "No hace falta descargar nada. También puede importar abajo los archivos del editor, por ejemplo una versión más reciente.",
    ),
    (
        "Generic foods such as fruit, eggs or milk are not included: no food composition table is installed.",
        "Les aliments génériques comme les fruits, les œufs ou le lait ne sont pas inclus : aucune table de composition n'est installée.",
        "Los alimentos genéricos como fruta, huevos o leche no se incluyen: no hay ninguna tabla de composición instalada.",
    ),
    (
        "Add a table in Settings",
        "Ajouter une table dans les Paramètres",
        "Añadir una tabla en Ajustes",
    ),
    (
        "Remove {0}? Foods you already saved from it are kept.",
        "Supprimer {0} ? Les aliments que vous en aviez déjà enregistrés sont conservés.",
        "¿Eliminar {0}? Los alimentos que ya guardó de ella se conservan.",
    ),
    (
        "{0} does not have the expected layout for {1}. The publisher may have changed its format.",
        "{0} n'a pas la structure attendue pour {1}. L'éditeur a peut-être changé son format.",
        "{0} no tiene la estructura esperada para {1}. Es posible que el editor haya cambiado su formato.",
    ),
    (
        "Files are read on this server, and searching these tables never contacts their publishers. Tables can also be imported from the command line, for example:",
        "Les fichiers sont lus sur ce serveur, et la recherche dans ces tables ne contacte jamais leurs éditeurs. Les tables peuvent aussi être importées en ligne de commande, par exemple :",
        "Los archivos se leen en este servidor y la búsqueda en estas tablas nunca contacta con sus editores. Las tablas también se pueden importar desde la línea de comandos, por ejemplo:",
    ),
    (
        "Recommended for your country",
        "Recommandée pour votre pays",
        "Recomendada para su país",
    ),
    (
        "McCance and Widdowson's Composition of Foods Integrated Dataset, Public Health England. Open Government Licence v3.0.",
        "McCance and Widdowson's Composition of Foods Integrated Dataset, Public Health England. Open Government Licence v3.0.",
        "McCance and Widdowson's Composition of Foods Integrated Dataset, Public Health England. Open Government Licence v3.0.",
    ),
    (
        "The file could not be opened. Upload the file exactly as downloaded for {0}.",
        "Le fichier n'a pas pu être ouvert. Envoyez le fichier tel qu'il a été téléchargé pour {0}.",
        "No se pudo abrir el archivo. Suba el archivo tal como se descargó para {0}.",
    ),
    (
        "The upload does not contain {0}. Check that you downloaded the right file for {1}.",
        "Le fichier envoyé ne contient pas {0}. Vérifiez que vous avez téléchargé le bon fichier pour {1}.",
        "El archivo subido no contiene {0}. Compruebe que descargó el archivo correcto para {1}.",
    ),
    (
        "USDA FoodData Central, U.S. Department of Agriculture, Agricultural Research Service. Public domain (CC0 1.0).",
        "USDA FoodData Central, ministère de l'Agriculture des États-Unis, Agricultural Research Service. Domaine public (CC0 1.0).",
        "USDA FoodData Central, Departamento de Agricultura de los Estados Unidos, Agricultural Research Service. Dominio público (CC0 1.0).",
    ),
    (
        "Choose a country from the list.",
        "Choisissez un pays dans la liste.",
        "Elija un país de la lista.",
    ),
    (
        "Set your country above so search uses the table that fits it best.",
        "Indiquez votre pays ci-dessus pour que la recherche utilise la table la mieux adaptée.",
        "Indique su país arriba para que la búsqueda use la tabla que mejor se adapte.",
    ),
    (
        "ANSES-CIQUAL French food composition table. Licence Ouverte / Open Licence (Etalab).",
        "Table de composition nutritionnelle des aliments ANSES-CIQUAL. Licence Ouverte / Open Licence (Etalab).",
        "Tabla francesa de composición de alimentos ANSES-CIQUAL. Licence Ouverte / Open Licence (Etalab).",
    ),
    (
        "Carbohydrate is the total by difference and includes fibre, unlike European labels.",
        "Les glucides sont le total calculé par différence et incluent les fibres, contrairement aux étiquettes européennes.",
        "Los carbohidratos son el total por diferencia e incluyen la fibra, a diferencia de las etiquetas europeas.",
    ),
    (
        "Published as traces or below the detection limit and recorded as 0: {0}.",
        "Publié en traces ou sous la limite de détection et enregistré comme 0 : {0}.",
        "Publicado como trazas o por debajo del límite de detección y registrado como 0: {0}.",
    ),
    (
        "Choose the downloaded file to import.",
        "Choisissez le fichier téléchargé à importer.",
        "Elija el archivo descargado que desea importar.",
    ),
    // Login and account.
    ("Log in", "Se connecter", "Iniciar sesión"),
    ("Log out", "Se déconnecter", "Cerrar sesión"),
    ("Account", "Compte", "Cuenta"),
    ("Username", "Nom d'utilisateur", "Nombre de usuario"),
    ("Password", "Mot de passe", "Contraseña"),
    (
        "Confirm password",
        "Confirmer le mot de passe",
        "Confirmar contraseña",
    ),
    (
        "Current password",
        "Mot de passe actuel",
        "Contraseña actual",
    ),
    ("New password", "Nouveau mot de passe", "Nueva contraseña"),
    (
        "Confirm new password",
        "Confirmer le nouveau mot de passe",
        "Confirmar nueva contraseña",
    ),
    (
        "Change password",
        "Changer le mot de passe",
        "Cambiar contraseña",
    ),
    (
        "Setup code",
        "Code de configuration",
        "Código de configuración",
    ),
    ("Create account", "Créer le compte", "Crear cuenta"),
    (
        "Create your account",
        "Créez votre compte",
        "Cree su cuenta",
    ),
    (
        "Korpus is protected by a login. Choose a username and password for this installation.",
        "Korpus est protégé par une connexion. Choisissez un nom d'utilisateur et un mot de passe pour cette installation.",
        "Korpus está protegido con un inicio de sesión. Elija un nombre de usuario y una contraseña para esta instalación.",
    ),
    (
        "Printed in the server log when Korpus starts. It proves that you are the one running this server. For example:",
        "Affiché dans le journal du serveur au démarrage de Korpus. Il prouve que c'est vous qui gérez ce serveur. Par exemple :",
        "Se muestra en el registro del servidor al iniciarse Korpus. Demuestra que usted administra este servidor. Por ejemplo:",
    ),
    (
        "At least {0} characters. A passphrase of several words is easy to remember and hard to guess.",
        "Au moins {0} caractères. Une phrase de plusieurs mots est facile à retenir et difficile à deviner.",
        "Al menos {0} caracteres. Una frase de varias palabras es fácil de recordar y difícil de adivinar.",
    ),
    (
        "Enter a username.",
        "Saisissez un nom d'utilisateur.",
        "Introduzca un nombre de usuario.",
    ),
    (
        "The username can have at most {0} characters.",
        "Le nom d'utilisateur peut comporter au plus {0} caractères.",
        "El nombre de usuario puede tener como máximo {0} caracteres.",
    ),
    (
        "The password must have at least {0} characters.",
        "Le mot de passe doit comporter au moins {0} caractères.",
        "La contraseña debe tener al menos {0} caracteres.",
    ),
    (
        "The password is too long.",
        "Le mot de passe est trop long.",
        "La contraseña es demasiado larga.",
    ),
    (
        "The passwords do not match.",
        "Les mots de passe ne correspondent pas.",
        "Las contraseñas no coinciden.",
    ),
    (
        "The setup code is incorrect. Check the server log.",
        "Le code de configuration est incorrect. Consultez le journal du serveur.",
        "El código de configuración es incorrecto. Consulte el registro del servidor.",
    ),
    (
        "The username or password is incorrect.",
        "Le nom d'utilisateur ou le mot de passe est incorrect.",
        "El nombre de usuario o la contraseña son incorrectos.",
    ),
    (
        "The current password is incorrect.",
        "Le mot de passe actuel est incorrect.",
        "La contraseña actual es incorrecta.",
    ),
    (
        "Too many failed attempts. Wait a few minutes and try again.",
        "Trop de tentatives échouées. Patientez quelques minutes puis réessayez.",
        "Demasiados intentos fallidos. Espere unos minutos e inténtelo de nuevo.",
    ),
    (
        "Forgot your password? The server administrator can remove the login with this command, then create a new one. Your diary is kept:",
        "Mot de passe oublié ? L'administrateur du serveur peut supprimer la connexion avec cette commande, puis en créer une nouvelle. Votre journal est conservé :",
        "¿Olvidó su contraseña? El administrador del servidor puede eliminar el inicio de sesión con este comando y crear uno nuevo. Su diario se conserva:",
    ),
    (
        "Two-factor authentication",
        "Authentification à deux facteurs",
        "Autenticación de dos factores",
    ),
    (
        "Authentication code",
        "Code d'authentification",
        "Código de autenticación",
    ),
    (
        "Enter the 6-digit code from your authenticator app.",
        "Saisissez le code à 6 chiffres de votre application d'authentification.",
        "Introduzca el código de 6 dígitos de su aplicación de autenticación.",
    ),
    (
        "Enter the 6-digit code the app shows.",
        "Saisissez le code à 6 chiffres affiché par l'application.",
        "Introduzca el código de 6 dígitos que muestra la aplicación.",
    ),
    (
        "The code is incorrect or has expired.",
        "Le code est incorrect ou a expiré.",
        "El código es incorrecto o ha caducado.",
    ),
    ("Verify", "Vérifier", "Verificar"),
    (
        "Back to log in",
        "Retour à la connexion",
        "Volver al inicio de sesión",
    ),
    (
        "Lost your authenticator? The server administrator can turn two-factor authentication off with:",
        "Application d'authentification perdue ? L'administrateur du serveur peut désactiver l'authentification à deux facteurs avec :",
        "¿Perdió su aplicación de autenticación? El administrador del servidor puede desactivar la autenticación de dos factores con:",
    ),
    ("On", "Activée", "Activada"),
    ("Off", "Désactivée", "Desactivada"),
    ("Turn on", "Activer", "Activar"),
    ("Turn off", "Désactiver", "Desactivar"),
    (
        "Turn off two-factor authentication?",
        "Désactiver l'authentification à deux facteurs ?",
        "¿Desactivar la autenticación de dos factores?",
    ),
    (
        "Logging in will only need your password.",
        "La connexion ne demandera plus que votre mot de passe.",
        "Para iniciar sesión solo se pedirá su contraseña.",
    ),
    (
        "Enter your password to turn off two-factor authentication.",
        "Saisissez votre mot de passe pour désactiver l'authentification à deux facteurs.",
        "Introduzca su contraseña para desactivar la autenticación de dos factores.",
    ),
    (
        "Enter your password to set up two-factor authentication.",
        "Saisissez votre mot de passe pour configurer l'authentification à deux facteurs.",
        "Introduzca su contraseña para configurar la autenticación de dos factores.",
    ),
    (
        "Start over with a new QR code",
        "Recommencer avec un nouveau code QR",
        "Empezar de nuevo con un nuevo código QR",
    ),
    (
        "Logging in asks for a code from your authenticator app after the password.",
        "La connexion demande un code de votre application d'authentification après le mot de passe.",
        "Al iniciar sesión se pide un código de su aplicación de autenticación después de la contraseña.",
    ),
    (
        "When it is on, logging in also asks for a code from an authenticator app on your phone, so a stolen password is not enough.",
        "Lorsqu'elle est activée, la connexion demande aussi un code d'une application d'authentification sur votre téléphone : un mot de passe volé ne suffit plus.",
        "Cuando está activada, al iniciar sesión también se pide un código de una aplicación de autenticación en su teléfono, así que una contraseña robada no basta.",
    ),
    (
        "Scan this QR code with an authenticator app, for example Aegis, 2FAS, Google Authenticator or 1Password.",
        "Scannez ce code QR avec une application d'authentification, par exemple Aegis, 2FAS, Google Authenticator ou 1Password.",
        "Escanee este código QR con una aplicación de autenticación, por ejemplo Aegis, 2FAS, Google Authenticator o 1Password.",
    ),
    (
        "Or enter this key manually:",
        "Ou saisissez cette clé manuellement :",
        "O introduzca esta clave manualmente:",
    ),
    (
        "QR code for your authenticator app",
        "Code QR pour votre application d'authentification",
        "Código QR para su aplicación de autenticación",
    ),
    (
        "Other devices are logged out.",
        "Les autres appareils sont déconnectés.",
        "Se cierra la sesión en los demás dispositivos.",
    ),
    (
        "Password changed. Other devices have been logged out.",
        "Mot de passe modifié. Les autres appareils ont été déconnectés.",
        "Contraseña cambiada. Se ha cerrado la sesión en los demás dispositivos.",
    ),
    (
        "Two-factor authentication is on. Other devices have been logged out.",
        "L'authentification à deux facteurs est activée. Les autres appareils ont été déconnectés.",
        "La autenticación de dos factores está activada. Se ha cerrado la sesión en los demás dispositivos.",
    ),
    (
        "Two-factor authentication is off.",
        "L'authentification à deux facteurs est désactivée.",
        "La autenticación de dos factores está desactivada.",
    ),
    ("Activity", "Activité", "Actividad"),
    (
        "Activity date",
        "Date de l'activité",
        "Fecha de la actividad",
    ),
    ("Steps", "Pas", "Pasos"),
    ("steps", "pas", "pasos"),
    ("Sport", "Sport", "Deporte"),
    ("{0} in total", "{0} au total", "{0} en total"),
    (
        "No sport on this day.",
        "Aucun sport ce jour-là.",
        "Ningún deporte este día.",
    ),
    ("Add sport", "Ajouter un sport", "Añadir deporte"),
    ("Choose", "Choisir", "Elegir"),
    ("Custom", "Personnalisé", "Personalizado"),
    ("Intensity", "Intensité", "Intensidad"),
    ("Easy", "Facile", "Suave"),
    ("Moderate", "Modérée", "Moderada"),
    ("Hard", "Intense", "Intensa"),
    (
        "Easy: breathing stays easy. Moderate: you can talk but not sing. Hard: you can only say a few words at a time.",
        "Facile : la respiration reste aisée. Modérée : vous pouvez parler, mais pas chanter. Intense : vous ne pouvez dire que quelques mots à la fois.",
        "Suave: respiras con facilidad. Moderada: puedes hablar, pero no cantar. Intensa: solo puedes decir unas pocas palabras seguidas.",
    ),
    (
        "Choose a sport.",
        "Choisissez un sport.",
        "Elige un deporte.",
    ),
    (
        "Enter the name of the sport.",
        "Saisissez le nom du sport.",
        "Introduce el nombre del deporte.",
    ),
    (
        "Choose how intense it is.",
        "Choisissez son intensité.",
        "Elige su intensidad.",
    ),
    (
        "+{0} kcal from {1} of sport",
        "+{0} kcal pour {1} de sport",
        "+{0} kcal por {1} de deporte",
    ),
    (
        "Add your weight to estimate the calories.",
        "Ajoutez votre poids pour estimer les calories.",
        "Añade tu peso para estimar las calorías.",
    ),
    (
        "Calories are estimated from the usual intensity of each sport, its duration and your weight, counting only what is spent above rest. If your step count already includes a walk or run listed here, it is counted twice.",
        "Les calories sont estimées d'après l'intensité habituelle de chaque sport, sa durée et votre poids, en ne comptant que la dépense au-delà du repos. Si votre nombre de pas inclut déjà une marche ou une course listée ici, elle est comptée deux fois.",
        "Las calorías se estiman según la intensidad habitual de cada deporte, su duración y tu peso, contando solo el gasto por encima del reposo. Si tu número de pasos ya incluye una caminata o carrera de esta lista, se cuenta dos veces.",
    ),
    ("sport", "sport", "deporte"),
    ("intensity", "intensité", "intensidad"),
    ("Duration", "Durée", "Duración"),
    ("minutes", "minutes", "minutos"),
    (
        "Repeat every week",
        "Répéter chaque semaine",
        "Repetir cada semana",
    ),
    ("Repeat on", "Répéter le", "Repetir el"),
    ("Until", "Jusqu'au", "Hasta"),
    (
        "A repeating activity starts on the date above and continues every week until the end date, if you set one.",
        "Une activité répétée commence à la date ci-dessus et continue chaque semaine jusqu'à la date de fin, si vous en choisissez une.",
        "Una actividad repetida empieza en la fecha indicada y continúa cada semana hasta la fecha de fin, si eliges una.",
    ),
    (
        "Back to activity",
        "Retour à l'activité",
        "Volver a actividad",
    ),
    (
        "Changes apply to every day this activity repeats on, including past days. To stop it from a given day, set an end date.",
        "Les modifications s'appliquent à chaque jour où cette activité se répète, jours passés compris. Pour l'arrêter à partir d'un jour donné, choisissez une date de fin.",
        "Los cambios se aplican a todos los días en que se repite esta actividad, incluidos los pasados. Para detenerla a partir de un día, elige una fecha de fin.",
    ),
    (
        "Skip on {0} only",
        "Ignorer le {0} seulement",
        "Omitir solo el {0}",
    ),
    (
        "Delete this activity?",
        "Supprimer cette activité ?",
        "¿Eliminar esta actividad?",
    ),
    (
        "It is removed from every day it repeats on, including past days.",
        "Elle est retirée de chaque jour où elle se répète, jours passés compris.",
        "Se elimina de todos los días en que se repite, incluidos los pasados.",
    ),
    (
        "Delete activity",
        "Supprimer l'activité",
        "Eliminar actividad",
    ),
    ("Every day", "Tous les jours", "Todos los días"),
    (
        "Every week: {0}",
        "Chaque semaine : {0}",
        "Cada semana: {0}",
    ),
    (
        "Enter a number of steps.",
        "Saisissez un nombre de pas.",
        "Introduce un número de pasos.",
    ),
    (
        "Enter at most {0} steps.",
        "Saisissez au plus {0} pas.",
        "Introduce como máximo {0} pasos.",
    ),
    (
        "Enter the steps as a whole number, for example 8000.",
        "Saisissez les pas en nombre entier, par exemple 8000.",
        "Introduce los pasos como número entero, por ejemplo 8000.",
    ),
    (
        "Enter how many minutes it lasted.",
        "Saisissez la durée en minutes.",
        "Introduce cuántos minutos duró.",
    ),
    (
        "Enter a duration between 1 and {0} minutes.",
        "Saisissez une durée entre 1 et {0} minutes.",
        "Introduce una duración entre 1 y {0} minutos.",
    ),
    (
        "Enter the duration in whole minutes, for example 45.",
        "Saisissez la durée en minutes entières, par exemple 45.",
        "Introduce la duración en minutos enteros, por ejemplo 45.",
    ),
    (
        "Choose at least one day of the week.",
        "Choisissez au moins un jour de la semaine.",
        "Elige al menos un día de la semana.",
    ),
    (
        "The end date must be on or after the first day.",
        "La date de fin doit être le premier jour ou après.",
        "La fecha de fin debe ser el primer día o posterior.",
    ),
    (
        "This activity does not repeat on that day.",
        "Cette activité ne se répète pas ce jour-là.",
        "Esta actividad no se repite ese día.",
    ),
    ("Steps saved.", "Pas enregistrés.", "Pasos guardados."),
    ("Activity added.", "Activité ajoutée.", "Actividad añadida."),
    (
        "Activity updated.",
        "Activité modifiée.",
        "Actividad actualizada.",
    ),
    (
        "Activity skipped on this day.",
        "Activité ignorée ce jour-là.",
        "Actividad omitida este día.",
    ),
    (
        "Activity deleted.",
        "Activité supprimée.",
        "Actividad eliminada.",
    ),
    ("steps for {0}", "pas du {0}", "pasos del {0}"),
    ("activity {0}", "activité {0}", "actividad {0}"),
    ("duration", "durée", "duración"),
    (
        "days of the week",
        "jours de la semaine",
        "días de la semana",
    ),
    (
        "unknown activity",
        "activité inconnue",
        "actividad desconocida",
    ),
    (
        "steps for {0} could not be stored ({1})",
        "les pas du {0} n'ont pas pu être enregistrés ({1})",
        "los pasos del {0} no se pudieron guardar ({1})",
    ),
    (
        "activity {0} could not be stored ({1})",
        "l'activité {0} n'a pas pu être enregistrée ({1})",
        "la actividad {0} no se pudo guardar ({1})",
    ),
    (
        "Steps that fill the ring on the Activity tab.",
        "Nombre de pas qui remplit l'anneau de l'onglet Activité.",
        "Pasos que llenan el anillo de la pestaña Actividad.",
    ),
    (
        "+{0} kcal from {1} steps",
        "+{0} kcal pour {1} pas",
        "+{0} kcal por {1} pasos",
    ),
    (
        "added to this day's calorie target",
        "ajoutées à l'objectif calorique du jour",
        "añadidas al objetivo calórico del día",
    ),
    ("Steps deleted.", "Pas supprimés.", "Pasos eliminados."),
    (
        "Calories are estimated from your height and weight and added to the day's target in the diary. If the activity level in Goals already includes this walking, it is counted twice; Sedentary avoids that.",
        "Les calories sont estimées d'après votre taille et votre poids, puis ajoutées à l'objectif du jour dans le journal. Si le niveau d'activité choisi dans Objectifs inclut déjà cette marche, elle est comptée deux fois ; « Sédentaire » l'évite.",
        "Las calorías se estiman a partir de tu altura y tu peso y se añaden al objetivo del día en el diario. Si el nivel de actividad de Objetivos ya incluye esta caminata, se cuenta dos veces; «Sedentario» lo evita.",
    ),
    (
        "Step goal saved.",
        "Objectif de pas enregistré.",
        "Objetivo de pasos guardado.",
    ),
    ("Steps walked", "Pas faits", "Pasos dados"),
    ("of {0} steps", "sur {0} pas", "de {0} pasos"),
    ("step goal", "objectif de pas", "objetivo de pasos"),
    ("Delete steps", "Supprimer les pas", "Eliminar los pasos"),
    (
        "Daily step goal",
        "Objectif de pas quotidien",
        "Objetivo diario de pasos",
    ),
    (
        "Record your steps at the end of the day.",
        "Enregistrez vos pas en fin de journée.",
        "Registra tus pasos al final del día.",
    ),
    (
        "Add your height and weight to estimate the calories.",
        "Indiquez votre taille et votre poids pour estimer les calories.",
        "Indica tu altura y tu peso para estimar las calorías.",
    ),
    (
        "Enter a step goal between {0} and {1}.",
        "Saisissez un objectif entre {0} et {1} pas.",
        "Introduce un objetivo entre {0} y {1} pasos.",
    ),
    ("Gym", "Muscu", "Gimnasio"),
    ("Programs", "Programmes", "Programas"),
    ("Other programs", "Autres programmes", "Otros programas"),
    ("New program", "Nouveau programme", "Nuevo programa"),
    ("Edit program", "Modifier le programme", "Editar programa"),
    ("Create program", "Créer le programme", "Crear programa"),
    (
        "Delete program",
        "Supprimer le programme",
        "Eliminar programa",
    ),
    (
        "Delete this program?",
        "Supprimer ce programme ?",
        "¿Eliminar este programa?",
    ),
    (
        "Its routines and their exercises are deleted with it.",
        "Ses séances et leurs exercices sont supprimés avec lui.",
        "Sus rutinas y sus ejercicios se eliminan con él.",
    ),
    ("Current program", "Programme actuel", "Programa actual"),
    (
        "Make this my current program",
        "En faire mon programme actuel",
        "Usar como mi programa actual",
    ),
    (
        "This is now your current program.",
        "C'est maintenant votre programme actuel.",
        "Ahora es tu programa actual.",
    ),
    ("Program created.", "Programme créé.", "Programa creado."),
    (
        "Program updated.",
        "Programme modifié.",
        "Programa actualizado.",
    ),
    (
        "Program deleted.",
        "Programme supprimé.",
        "Programa eliminado.",
    ),
    (
        "Back to program",
        "Retour au programme",
        "Volver al programa",
    ),
    ("Back to gym", "Retour à la muscu", "Volver al gimnasio"),
    (
        "Rename or change schedule",
        "Renommer ou changer la planification",
        "Renombrar o cambiar la planificación",
    ),
    (
        "You can keep up to {0} programs. Delete one you no longer use first.",
        "Vous pouvez garder jusqu'à {0} programmes. Supprimez d'abord celui que vous n'utilisez plus.",
        "Puedes tener hasta {0} programas. Elimina primero uno que ya no uses.",
    ),
    (
        "A program groups the routines you train, such as Push, Pull and Legs, each with its exercises, sets and reps.",
        "Un programme regroupe vos séances, par exemple Poussée, Tirage et Jambes, chacune avec ses exercices, séries et répétitions.",
        "Un programa agrupa tus rutinas, como Empuje, Tirón y Pierna, cada una con sus ejercicios, series y repeticiones.",
    ),
    (
        "Start from a template",
        "Partir d'un modèle",
        "Empezar con una plantilla",
    ),
    (
        "Or start from a template",
        "Ou partir d'un modèle",
        "O empezar con una plantilla",
    ),
    (
        "A template becomes an ordinary program: rename it, change its days and exercises, and set your own weights.",
        "Un modèle devient un programme comme les autres : renommez-le, changez ses jours et ses exercices, et indiquez vos propres charges.",
        "Una plantilla se convierte en un programa normal: cámbiale el nombre, los días y los ejercicios, e indica tus propios pesos.",
    ),
    ("No exercises", "Aucun exercice", "Ningún ejercicio"),
    ("No routines", "Aucune séance", "Ninguna rutina"),
    (
        "Push / Pull / Legs",
        "Poussée / Tirage / Jambes",
        "Empuje / Tirón / Pierna",
    ),
    ("Upper / Lower", "Haut / Bas du corps", "Torso / Pierna"),
    ("Full body", "Corps entier", "Cuerpo completo"),
    (
        "Chest, shoulders and triceps; back and biceps; legs. 3 days a week.",
        "Pectoraux, épaules et triceps ; dos et biceps ; jambes. 3 jours par semaine.",
        "Pecho, hombros y tríceps; espalda y bíceps; pierna. 3 días por semana.",
    ),
    (
        "Upper body and lower body, each twice a week. 4 days a week.",
        "Haut et bas du corps, chacun deux fois par semaine. 4 jours par semaine.",
        "Torso y pierna, cada uno dos veces por semana. 4 días por semana.",
    ),
    (
        "The whole body in every session, three variations. 3 days a week.",
        "Tout le corps à chaque séance, en trois variantes. 3 jours par semaine.",
        "Todo el cuerpo en cada sesión, en tres variantes. 3 días por semana.",
    ),
    (
        "Five sets of five on the main barbell lifts. 3 days a week.",
        "Cinq séries de cinq sur les principaux mouvements à la barre. 3 jours par semaine.",
        "Cinco series de cinco en los principales ejercicios con barra. 3 días por semana.",
    ),
    ("Push", "Poussée", "Empuje"),
    ("Pull", "Tirage", "Tirón"),
    ("Legs", "Jambes", "Pierna"),
    ("Upper body A", "Haut du corps A", "Torso A"),
    ("Upper body B", "Haut du corps B", "Torso B"),
    ("Lower body A", "Bas du corps A", "Pierna A"),
    ("Lower body B", "Bas du corps B", "Pierna B"),
    ("Full body A", "Corps entier A", "Cuerpo completo A"),
    ("Full body B", "Corps entier B", "Cuerpo completo B"),
    ("Full body C", "Corps entier C", "Cuerpo completo C"),
    ("Workout A", "Séance A", "Sesión A"),
    ("Workout B", "Séance B", "Sesión B"),
    ("Schedule", "Planification", "Planificación"),
    (
        "Days of the week",
        "Jours de la semaine",
        "Días de la semana",
    ),
    ("Rotation", "Rotation", "Rotación"),
    (
        "Choose how the routines are scheduled.",
        "Choisissez comment les séances sont planifiées.",
        "Elige cómo se planifican las rutinas.",
    ),
    (
        "Days of the week: each routine has its days, such as Push on Monday.",
        "Jours de la semaine : chaque séance a ses jours, par exemple Poussée le lundi.",
        "Días de la semana: cada rutina tiene sus días, por ejemplo Empuje el lunes.",
    ),
    (
        "Rotation: the routines follow each other in order, A, B, C, then A again, whatever the day you train.",
        "Rotation : les séances se suivent dans l'ordre, A, B, C, puis de nouveau A, quel que soit le jour où vous vous entraînez.",
        "Rotación: las rutinas se suceden en orden, A, B, C y otra vez A, sea cual sea el día en que entrenes.",
    ),
    ("e.g. Strength", "p. ex. Force", "p. ej., Fuerza"),
    ("Routines", "Séances", "Rutinas"),
    ("1 routine", "1 séance", "1 rutina"),
    ("{0} routines", "{0} séances", "{0} rutinas"),
    ("Add a routine", "Ajouter une séance", "Añadir una rutina"),
    ("Add routine", "Ajouter la séance", "Añadir rutina"),
    ("Edit routine", "Modifier la séance", "Editar rutina"),
    ("Delete routine", "Supprimer la séance", "Eliminar rutina"),
    (
        "Delete this routine?",
        "Supprimer cette séance ?",
        "¿Eliminar esta rutina?",
    ),
    (
        "Its exercises are removed from the program.",
        "Ses exercices sont retirés du programme.",
        "Sus ejercicios se quitan del programa.",
    ),
    (
        "Routine created. Add its exercises.",
        "Séance créée. Ajoutez ses exercices.",
        "Rutina creada. Añade sus ejercicios.",
    ),
    (
        "Routine updated.",
        "Séance modifiée.",
        "Rutina actualizada.",
    ),
    ("Routine deleted.", "Séance supprimée.", "Rutina eliminada."),
    (
        "Back to routine",
        "Retour à la séance",
        "Volver a la rutina",
    ),
    (
        "Rename or change days",
        "Renommer ou changer les jours",
        "Renombrar o cambiar los días",
    ),
    (
        "A program can hold up to {0} routines.",
        "Un programme peut contenir jusqu'à {0} séances.",
        "Un programa puede tener hasta {0} rutinas.",
    ),
    (
        "No routines yet. Add one below, such as Push, Pull or Legs.",
        "Aucune séance pour l'instant. Ajoutez-en une ci-dessous, par exemple Poussée, Tirage ou Jambes.",
        "Aún no hay rutinas. Añade una abajo, como Empuje, Tirón o Pierna.",
    ),
    (
        "This program has no routines yet.",
        "Ce programme n'a pas encore de séance.",
        "Este programa aún no tiene rutinas.",
    ),
    (
        "The routines follow each other in this order, then start again from the first.",
        "Les séances se suivent dans cet ordre, puis on recommence à la première.",
        "Las rutinas se suceden en este orden y luego se vuelve a empezar por la primera.",
    ),
    (
        "Do the routines in this order, then start again from the first. Each session is the next routine, whatever the day.",
        "Faites les séances dans cet ordre, puis recommencez à la première. Chaque entraînement est la séance suivante, quel que soit le jour.",
        "Haz las rutinas en este orden y luego vuelve a empezar por la primera. Cada sesión es la rutina siguiente, sea cual sea el día.",
    ),
    (
        "This program is a rotation, so routines have no days. Change the order on the program page.",
        "Ce programme est une rotation : les séances n'ont pas de jours. Changez l'ordre sur la page du programme.",
        "Este programa es una rotación: las rutinas no tienen días. Cambia el orden en la página del programa.",
    ),
    (
        "Session {0} of the rotation",
        "Séance {0} de la rotation",
        "Sesión {0} de la rotación",
    ),
    (
        "Not on a day yet",
        "Sans jour pour l'instant",
        "Aún sin día",
    ),
    ("No day chosen", "Aucun jour choisi", "Ningún día elegido"),
    ("Days", "Jours", "Días"),
    ("e.g. Push", "p. ex. Poussée", "p. ej., Empuje"),
    ("Rest", "Repos", "Descanso"),
    ("Move {0} up", "Monter {0}", "Subir {0}"),
    ("Move {0} down", "Descendre {0}", "Bajar {0}"),
    ("Exercises", "Exercices", "Ejercicios"),
    ("1 exercise", "1 exercice", "1 ejercicio"),
    ("{0} exercises", "{0} exercices", "{0} ejercicios"),
    ("Add exercise", "Ajouter un exercice", "Añadir ejercicio"),
    (
        "Back to exercises",
        "Retour aux exercices",
        "Volver a los ejercicios",
    ),
    (
        "Browse the exercise list and add your own",
        "Parcourez la liste des exercices et ajoutez les vôtres",
        "Consulta la lista de ejercicios y añade los tuyos",
    ),
    (
        "Search exercises",
        "Rechercher un exercice",
        "Buscar ejercicios",
    ),
    ("All", "Tous", "Todos"),
    ("Yours", "Le vôtre", "Tuyo"),
    (
        "No exercise matches. You can create it below.",
        "Aucun exercice ne correspond. Vous pouvez le créer ci-dessous.",
        "Ningún ejercicio coincide. Puedes crearlo abajo.",
    ),
    (
        "Add the exercise yourself with its muscle group and equipment.",
        "Ajoutez l'exercice vous-même avec son groupe musculaire et son matériel.",
        "Añade tú mismo el ejercicio con su grupo muscular y su material.",
    ),
    (
        "Create an exercise",
        "Créer un exercice",
        "Crear un ejercicio",
    ),
    (
        "Choose exercises from the list, then set their sets, reps, weight and rest.",
        "Choisissez des exercices dans la liste, puis indiquez leurs séries, répétitions, charge et repos.",
        "Elige ejercicios de la lista e indica sus series, repeticiones, peso y descanso.",
    ),
    (
        "Unknown exercise",
        "Exercice inconnu",
        "Ejercicio desconocido",
    ),
    ("Sets", "Séries", "Series"),
    ("{0} sets", "{0} séries", "{0} series"),
    ("Reps", "Répétitions", "Repeticiones"),
    ("1 rep", "1 répétition", "1 repetición"),
    ("{0} reps", "{0} répétitions", "{0} repeticiones"),
    ("Time per set", "Durée par série", "Tiempo por serie"),
    ("Added weight", "Lest", "Lastre"),
    (
        "Rest between sets",
        "Repos entre les séries",
        "Descanso entre series",
    ),
    ("Rest {0}", "Repos {0}", "Descanso {0}"),
    (
        "e.g. seat height 4",
        "p. ex. siège en position 4",
        "p. ej., asiento en posición 4",
    ),
    (
        "A number such as 8, or a range such as 8-12: add reps until you reach the top, then add weight.",
        "Un nombre comme 8, ou une fourchette comme 8-12 : ajoutez des répétitions jusqu'au haut de la fourchette, puis augmentez la charge.",
        "Un número como 8 o un rango como 8-12: añade repeticiones hasta llegar al máximo y luego sube el peso.",
    ),
    (
        "Times in seconds, or minutes and seconds such as 1:30.",
        "Durées en secondes, ou en minutes et secondes comme 1:30.",
        "Tiempos en segundos, o en minutos y segundos como 1:30.",
    ),
    (
        "Leave the weight empty when you use only your body weight.",
        "Laissez le lest vide si vous utilisez seulement le poids du corps.",
        "Deja el lastre vacío si usas solo tu peso corporal.",
    ),
    (
        "Add to routine",
        "Ajouter à la séance",
        "Añadir a la rutina",
    ),
    (
        "Remove from routine",
        "Retirer de la séance",
        "Quitar de la rutina",
    ),
    (
        "Remove this exercise from the routine?",
        "Retirer cet exercice de la séance ?",
        "¿Quitar este ejercicio de la rutina?",
    ),
    (
        "Exercise added to the routine.",
        "Exercice ajouté à la séance.",
        "Ejercicio añadido a la rutina.",
    ),
    (
        "Exercise updated.",
        "Exercice modifié.",
        "Ejercicio actualizado.",
    ),
    (
        "Exercise removed from the routine.",
        "Exercice retiré de la séance.",
        "Ejercicio quitado de la rutina.",
    ),
    (
        "A routine can hold up to {0} exercises.",
        "Une séance peut contenir jusqu'à {0} exercices.",
        "Una rutina puede tener hasta {0} ejercicios.",
    ),
    (
        "Enter the number of sets.",
        "Indiquez le nombre de séries.",
        "Indica el número de series.",
    ),
    (
        "Enter between 1 and {0} sets.",
        "Indiquez entre 1 et {0} séries.",
        "Indica entre 1 y {0} series.",
    ),
    (
        "Enter the reps, e.g. 8 or 8-12.",
        "Indiquez les répétitions, p. ex. 8 ou 8-12.",
        "Indica las repeticiones, p. ej., 8 u 8-12.",
    ),
    (
        "Enter reps between 1 and {0}, e.g. 8 or 8-12.",
        "Indiquez entre 1 et {0} répétitions, p. ex. 8 ou 8-12.",
        "Indica entre 1 y {0} repeticiones, p. ej., 8 u 8-12.",
    ),
    (
        "Enter a number of reps or a range from low to high, e.g. 8 or 8-12.",
        "Indiquez un nombre de répétitions ou une fourchette du plus petit au plus grand, p. ex. 8 ou 8-12.",
        "Indica un número de repeticiones o un rango de menor a mayor, p. ej., 8 u 8-12.",
    ),
    (
        "Enter the time of each set, e.g. 45 or 1:30.",
        "Indiquez la durée de chaque série, p. ex. 45 ou 1:30.",
        "Indica el tiempo de cada serie, p. ej., 45 o 1:30.",
    ),
    (
        "Enter a time up to 60 minutes, e.g. 45 or 1:30.",
        "Indiquez une durée de 60 minutes au plus, p. ex. 45 ou 1:30.",
        "Indica un tiempo de 60 minutos como máximo, p. ej., 45 o 1:30.",
    ),
    (
        "Enter a rest time up to 15 minutes, e.g. 90 or 1:30.",
        "Indiquez un repos de 15 minutes au plus, p. ex. 90 ou 1:30.",
        "Indica un descanso de 15 minutos como máximo, p. ej., 90 o 1:30.",
    ),
    (
        "Enter a weight between 0 and {0} {1}.",
        "Indiquez une charge entre 0 et {0} {1}.",
        "Indica un peso entre 0 y {0} {1}.",
    ),
    ("New exercise", "Nouvel exercice", "Nuevo ejercicio"),
    ("Edit exercise", "Modifier l'exercice", "Editar ejercicio"),
    ("Create exercise", "Créer l'exercice", "Crear ejercicio"),
    (
        "Delete exercise",
        "Supprimer l'exercice",
        "Eliminar ejercicio",
    ),
    (
        "Delete this exercise?",
        "Supprimer cet exercice ?",
        "¿Eliminar este ejercicio?",
    ),
    ("Exercise created.", "Exercice créé.", "Ejercicio creado."),
    (
        "Exercise saved.",
        "Exercice enregistré.",
        "Ejercicio guardado.",
    ),
    (
        "Exercise deleted.",
        "Exercice supprimé.",
        "Ejercicio eliminado.",
    ),
    ("Muscle group", "Groupe musculaire", "Grupo muscular"),
    ("Equipment", "Matériel", "Material"),
    (
        "Choose a muscle group.",
        "Choisissez un groupe musculaire.",
        "Elige un grupo muscular.",
    ),
    (
        "Choose the equipment.",
        "Choisissez le matériel.",
        "Elige el material.",
    ),
    (
        "Sets are held for a time",
        "Séries en durée",
        "Series por tiempo",
    ),
    (
        "For exercises such as a plank: each set is a time instead of a number of reps.",
        "Pour des exercices comme le gainage : chaque série est une durée plutôt qu'un nombre de répétitions.",
        "Para ejercicios como la plancha: cada serie es un tiempo en lugar de un número de repeticiones.",
    ),
    (
        "Used in a routine: whether sets are timed can no longer change.",
        "Utilisé dans une séance : le type de série ne peut plus changer.",
        "Se usa en una rutina: el tipo de serie ya no puede cambiar.",
    ),
    (
        "This exercise is used in a routine. Remove it from your routines first.",
        "Cet exercice est utilisé dans une séance. Retirez-le d'abord de vos séances.",
        "Este ejercicio se usa en una rutina. Quítalo primero de tus rutinas.",
    ),
    (
        "You can add up to {0} exercises of your own.",
        "Vous pouvez ajouter jusqu'à {0} exercices personnels.",
        "Puedes añadir hasta {0} ejercicios propios.",
    ),
    ("Chest", "Pectoraux", "Pecho"),
    ("Back", "Dos", "Espalda"),
    ("Shoulders", "Épaules", "Hombros"),
    ("Biceps", "Biceps", "Bíceps"),
    ("Triceps", "Triceps", "Tríceps"),
    ("Forearms", "Avant-bras", "Antebrazos"),
    ("Abs", "Abdominaux", "Abdominales"),
    ("Glutes", "Fessiers", "Glúteos"),
    ("Quadriceps", "Quadriceps", "Cuádriceps"),
    ("Hamstrings", "Ischio-jambiers", "Isquiotibiales"),
    ("Calves", "Mollets", "Gemelos"),
    ("Cardio", "Cardio", "Cardio"),
    ("Barbell", "Barre", "Barra"),
    ("Dumbbells", "Haltères", "Mancuernas"),
    ("EZ bar", "Barre EZ", "Barra Z"),
    ("Kettlebell", "Kettlebell", "Kettlebell"),
    ("Cable", "Poulie", "Polea"),
    ("Machine", "Machine", "Máquina"),
    ("Smith machine", "Barre guidée (Smith)", "Máquina Smith"),
    ("Body weight", "Poids du corps", "Peso corporal"),
    ("Resistance band", "Élastique", "Banda elástica"),
    ("Other", "Autre", "Otro"),
    ("program {0}", "programme {0}", "programa {0}"),
    ("routine {0}", "séance {0}", "rutina {0}"),
    (
        "routine exercise {0}",
        "exercice de séance {0}",
        "ejercicio de rutina {0}",
    ),
    ("exercise {0}", "exercice {0}", "ejercicio {0}"),
    (
        "unknown program",
        "programme inconnu",
        "programa desconocido",
    ),
    ("unknown routine", "séance inconnue", "rutina desconocida"),
    (
        "unknown exercise",
        "exercice inconnu",
        "ejercicio desconocido",
    ),
    (
        "more than one current program",
        "plusieurs programmes actuels",
        "más de un programa actual",
    ),
    (
        "sets, reps, weight or rest",
        "séries, répétitions, charge ou repos",
        "series, repeticiones, peso o descanso",
    ),
    (
        "too many gym records",
        "trop de données de muscu",
        "demasiados datos de gimnasio",
    ),
    (
        "No exercises in this workout.",
        "Aucun exercice dans cet entraînement.",
        "No hay ejercicios en este entrenamiento.",
    ),
    ("Last time", "Dernière fois", "Última vez"),
    (
        "Finish workout",
        "Terminer l'entraînement",
        "Terminar entrenamiento",
    ),
    ("Rest over", "Repos terminé", "Descanso terminado"),
    ("1 set", "1 série", "1 serie"),
    ("Newer", "Plus récents", "Más recientes"),
    ("Older", "Plus anciens", "Más antiguos"),
    (
        "Start an empty workout",
        "Commencer un entraînement libre",
        "Empezar un entrenamiento libre",
    ),
    (
        "Workout saved.",
        "Entraînement enregistré.",
        "Entrenamiento guardado.",
    ),
    (
        "workout set {0}",
        "série d'entraînement {0}",
        "serie de entrenamiento {0}",
    ),
    ("Plan", "Prévu", "Previsto"),
    ("Remove set", "Retirer une série", "Quitar una serie"),
    (
        "Choose a date that is not in the future.",
        "Choisissez une date qui n'est pas dans le futur.",
        "Elige una fecha que no sea futura.",
    ),
    (
        "{0}, set {1}: {2}",
        "{0}, série {1} : {2}",
        "{0}, serie {1}: {2}",
    ),
    (
        "date, name or routine",
        "date, nom ou séance",
        "fecha, nombre o rutina",
    ),
    (
        "Discard this workout?",
        "Abandonner cet entraînement ?",
        "¿Descartar este entrenamiento?",
    ),
    (
        "Some values could not be saved. Check them, then save.",
        "Certaines valeurs n'ont pas pu être enregistrées. Vérifiez-les, puis enregistrez.",
        "Algunos valores no se pudieron guardar. Revísalos y guarda.",
    ),
    ("Set", "Série", "Serie"),
    (
        "workout exercise {0}",
        "exercice d'entraînement {0}",
        "ejercicio de entrenamiento {0}",
    ),
    (
        "Workout discarded.",
        "Entraînement abandonné.",
        "Entrenamiento descartado.",
    ),
    (
        "Start workout",
        "Commencer l'entraînement",
        "Empezar entrenamiento",
    ),
    (
        "Delete this workout?",
        "Supprimer cet entraînement ?",
        "¿Eliminar este entrenamiento?",
    ),
    ("See all", "Tout voir", "Ver todo"),
    (
        "No exercises yet. Add the first one below.",
        "Aucun exercice pour l'instant. Ajoutez le premier ci-dessous.",
        "Aún no hay ejercicios. Añade el primero abajo.",
    ),
    (
        "A workout is already in progress. Finish or discard it first.",
        "Un entraînement est déjà en cours. Terminez-le ou abandonnez-le d'abord.",
        "Ya hay un entrenamiento en curso. Termínalo o descártalo primero.",
    ),
    (
        "more than one workout in progress",
        "plus d'un entraînement en cours",
        "más de un entrenamiento en curso",
    ),
    (
        "Edit workout",
        "Modifier l'entraînement",
        "Editar entrenamiento",
    ),
    (
        "A workout is in progress.",
        "Un entraînement est en cours.",
        "Hay un entrenamiento en curso.",
    ),
    ("Discard", "Abandonner", "Descartar"),
    ("Unknown action.", "Action inconnue.", "Acción desconocida."),
    ("Skip", "Passer", "Saltar"),
    (
        "unknown workout exercise",
        "exercice d'entraînement inconnu",
        "ejercicio de entrenamiento desconocido",
    ),
    ("Done", "Fait", "Hecho"),
    (
        "Enter the reps you did.",
        "Indiquez les répétitions effectuées.",
        "Indica las repeticiones que hiciste.",
    ),
    (
        "Sets you leave unticked are dropped when you finish.",
        "Les séries non cochées sont retirées quand vous terminez.",
        "Las series sin marcar se quitan al terminar.",
    ),
    ("Pages", "Pages", "Páginas"),
    (
        "Workout deleted.",
        "Entraînement supprimé.",
        "Entrenamiento eliminado.",
    ),
    (
        "Add 15 seconds",
        "Ajouter 15 secondes",
        "Añadir 15 segundos",
    ),
    (
        "Recent workouts",
        "Entraînements récents",
        "Entrenamientos recientes",
    ),
    (
        "The sets you logged in it are deleted.",
        "Les séries que vous y avez notées sont supprimées.",
        "Se eliminan las series que registraste en él.",
    ),
    ("Start {0}", "Commencer {0}", "Empezar {0}"),
    (
        "Enter the time you held, e.g. 45 or 1:30.",
        "Indiquez la durée tenue, p. ex. 45 ou 1:30.",
        "Indica el tiempo que aguantaste, p. ej., 45 o 1:30.",
    ),
    ("Time", "Durée", "Tiempo"),
    (
        "No finished workouts yet. Start one from a routine or from the Gym page.",
        "Aucun entraînement terminé pour l'instant. Commencez-en un depuis une séance ou la page Muscu.",
        "Aún no hay entrenamientos terminados. Empieza uno desde una rutina o desde la página Gimnasio.",
    ),
    (
        "reps, time or weight",
        "répétitions, durée ou poids",
        "repeticiones, tiempo o peso",
    ),
    (
        "Next in the rotation",
        "Suivante dans la rotation",
        "Siguiente en la rotación",
    ),
    (
        "A workout can hold up to {0} exercises.",
        "Un entraînement peut contenir jusqu'à {0} exercices.",
        "Un entrenamiento puede tener hasta {0} ejercicios.",
    ),
    ("Workout", "Entraînement", "Entrenamiento"),
    (
        "Delete workout",
        "Supprimer l'entraînement",
        "Eliminar entrenamiento",
    ),
    (
        "Workout finished.",
        "Entraînement terminé.",
        "Entrenamiento terminado.",
    ),
    ("Volume", "Volume", "Volumen"),
    (
        "Save and continue later",
        "Enregistrer et reprendre plus tard",
        "Guardar y seguir más tarde",
    ),
    (
        "Workout history",
        "Historique des entraînements",
        "Historial de entrenamientos",
    ),
    (
        "Last time you reached the top of the range on every set. You could add a little weight.",
        "La dernière fois, vous avez atteint le haut de la fourchette à chaque série. Vous pourriez ajouter un peu de poids.",
        "La última vez llegaste al máximo del rango en todas las series. Podrías añadir un poco de peso.",
    ),
    (
        "Enter a whole number of reps up to {0}.",
        "Indiquez un nombre entier de répétitions, jusqu'à {0}.",
        "Indica un número entero de repeticiones, hasta {0}.",
    ),
    (
        "Back to workout",
        "Retour à l'entraînement",
        "Volver al entrenamiento",
    ),
    ("Add set", "Ajouter une série", "Añadir serie"),
    (
        "Discard workout",
        "Abandonner l'entraînement",
        "Descartar entrenamiento",
    ),
    ("Continue it", "Le reprendre", "Continuarlo"),
    (
        "Mark at least one set as done to finish the workout, or discard it.",
        "Cochez au moins une série pour terminer l'entraînement, ou abandonnez-le.",
        "Marca al menos una serie como hecha para terminar el entrenamiento, o descártalo.",
    ),
    ("workout {0}", "entraînement {0}", "entrenamiento {0}"),
    (
        "unknown workout",
        "entraînement inconnu",
        "entrenamiento desconocido",
    ),
    ("None", "Aucune", "Ninguna"),
    ("In progress", "En cours", "En curso"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::Path;

    #[test]
    fn negotiates_accept_language() {
        assert_eq!(
            Lang::from_accept_language("fr-FR,fr;q=0.9,en;q=0.8"),
            Lang::Fr
        );
        assert_eq!(
            Lang::from_accept_language("de-DE,es;q=0.5,en;q=0.4"),
            Lang::Es
        );
        assert_eq!(Lang::from_accept_language("en;q=0.5, es;q=0.9"), Lang::Es);
        assert_eq!(Lang::from_accept_language("de, it"), Lang::En);
        assert_eq!(Lang::from_accept_language("fr;q=0"), Lang::En);
        assert_eq!(Lang::from_accept_language(""), Lang::En);
    }

    #[test]
    fn translates_and_formats() {
        assert_eq!(Lang::En.t("Diary"), "Diary");
        assert_eq!(Lang::Fr.t("Diary"), "Journal");
        assert_eq!(Lang::Es.t("Diary"), "Diario");
        assert_eq!(Lang::Fr.t("not a key"), "not a key");
        assert_eq!(
            Lang::Es.f("Delete {0} from {1}?", &[&"Avena", &"Desayuno"]),
            "¿Eliminar Avena de Desayuno?"
        );
        assert_eq!(Lang::Fr.num("12.5".into()), "12,5");
        assert_eq!(Lang::En.num("12.5".into()), "12.5");
    }

    #[test]
    fn catalog_has_no_duplicates_and_matching_placeholders() {
        let mut seen = HashSet::new();
        for (en, fr, es) in CATALOG {
            assert!(seen.insert(*en), "duplicate key: {en}");
            for i in 0..5 {
                let p = format!("{{{i}}}");
                let n = en.matches(&p).count();
                assert_eq!(fr.matches(&p).count(), n, "fr placeholder {p} in: {en}");
                assert_eq!(es.matches(&p).count(), n, "es placeholder {p} in: {en}");
            }
            assert!(!fr.is_empty() && !es.is_empty(), "empty translation: {en}");
        }
    }

    /// String literals passed to `.t("…")`, `.f("…"`, `.f1("…"` or `.f2("…"`
    /// anywhere in the code.
    fn used_keys(text: &str, out: &mut Vec<String>) {
        for marker in [".t(", ".f(", ".f1(", ".f2("] {
            let mut rest = text;
            while let Some(pos) = rest.find(marker) {
                rest = rest[pos + marker.len()..].trim_start();
                // Skip calls whose first argument is not a string literal.
                let Some(after_quote) = rest.strip_prefix('"') else {
                    continue;
                };
                rest = after_quote;
                let mut key = String::new();
                let mut chars = rest.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => {
                            if let Some(n) = chars.next() {
                                key.push(n);
                            }
                        }
                        c => key.push(c),
                    }
                }
                out.push(key);
            }
        }
    }

    fn collect(dir: &Path, ext: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, ext, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some(ext)
                && path.file_name().and_then(|n| n.to_str()) != Some("i18n.rs")
            {
                used_keys(&std::fs::read_to_string(&path).unwrap(), out);
            }
        }
    }

    fn used_catalog_keys() -> Vec<String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut keys = Vec::new();
        collect(&root.join("src"), "rs", &mut keys);
        collect(&root.join("templates"), "html", &mut keys);
        keys
    }

    #[test]
    fn catalog_has_no_unused_entries() {
        let used: HashSet<String> = used_catalog_keys().into_iter().collect();
        let unused: Vec<&str> = CATALOG
            .iter()
            .map(|(en, _, _)| *en)
            .filter(|en| !used.contains(*en))
            .collect();
        assert!(unused.is_empty(), "unused translations: {unused:#?}");
    }

    #[test]
    fn every_used_string_is_translated() {
        let keys = used_catalog_keys();
        assert!(
            keys.len() > 100,
            "key scan found too little: {}",
            keys.len()
        );
        let missing: Vec<&String> = keys
            .iter()
            .filter(|k| !catalog().contains_key(k.as_str()))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        assert!(missing.is_empty(), "untranslated strings: {missing:#?}");
    }
}
