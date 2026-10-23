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
    (
        "Import is only possible into an empty installation. This one already contains foods, diary entries or weights.",
        "L'import n'est possible que dans une installation vide. Celle-ci contient déjà des aliments, des entrées de journal ou des poids.",
        "La importación solo es posible en una instalación vacía. Esta ya contiene alimentos, entradas del diario o pesos.",
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
    (
        "Product saved to your foods.",
        "Produit ajouté à vos aliments.",
        "Producto guardado en sus alimentos.",
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
    ("Search: {0}", "Recherche : {0}", "Búsqueda: {0}"),
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
    ("Weight ({0})", "Poids ({0})", "Peso ({0})"),
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
        "{0} compared with goal",
        "{0} par rapport à l'objectif",
        "{0} respecto al objetivo",
    ),
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
    ("Amount ({0})", "Quantité ({0})", "Cantidad ({0})"),
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
        "Only possible while this installation has no foods, diary entries or weights.",
        "Possible uniquement tant que cette installation ne contient ni aliments, ni entrées de journal, ni poids.",
        "Solo es posible mientras esta instalación no tenga alimentos, entradas del diario ni pesos.",
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
    ("Your foods", "Vos aliments", "Sus alimentos"),
    (
        "You have not saved any foods yet.",
        "Vous n'avez encore enregistré aucun aliment.",
        "Todavía no ha guardado ningún alimento.",
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
    ("Height ({0})", "Taille ({0})", "Altura ({0})"),
    (
        "Suggested target ({0})",
        "Objectif suggéré ({0})",
        "Objetivo sugerido ({0})",
    ),
    ("{0} (g per day)", "{0} (g par jour)", "{0} (g por día)"),
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
        "Download the XML zip file of the Ciqual table.",
        "Téléchargez le fichier zip XML de la table Ciqual.",
        "Descargue el archivo zip XML de la tabla Ciqual.",
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
