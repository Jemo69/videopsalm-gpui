// Minimal i18n infra: en + fr dictionaries, t() lookup with fallback.

export type Lang = "en" | "fr";

const STRINGS: Record<Lang, Record<string, string>> = {
  en: {
    "app.title": "VideoPsalm (Electron, clean-room v1)",
    "tab.agenda": "Agenda",
    "tab.songs": "Songs",
    "tab.bible": "Bible",
    "tab.media": "Media & Style",
    "project": "Project",
    "stop": "Stop",
    "stage": "Stage view",
    "alert.show": "Show alert",
    "alert.hide": "Hide alert",
    "clock.start": "Start",
    "clock.reset": "Reset",
    "search.placeholder": "Instant find: title, lyrics, author, CCLI, id…",
    "agenda.new": "New agenda",
    "agenda.open": "Open",
    "agenda.save": "Save",
    "projector.empty": "Nothing projected — select an item and press Project.",
  },
  fr: {
    "app.title": "VideoPsalm (Electron, réécriture v1)",
    "tab.agenda": "Ordre du culte",
    "tab.songs": "Chants",
    "tab.bible": "Bible",
    "tab.media": "Médias & Style",
    "project": "Projeter",
    "stop": "Arrêter",
    "stage": "Retour scène",
    "alert.show": "Afficher l'alerte",
    "alert.hide": "Masquer l'alerte",
    "clock.start": "Démarrer",
    "clock.reset": "Réinitialiser",
    "search.placeholder": "Recherche instantanée : titre, paroles, auteur, CCLI, n°…",
    "agenda.new": "Nouvel ordre",
    "agenda.open": "Ouvrir",
    "agenda.save": "Enregistrer",
    "projector.empty": "Rien en projection — choisissez un élément puis Projeter.",
  },
};

export function t(lang: Lang, key: string): string {
  return STRINGS[lang][key] ?? STRINGS.en[key] ?? key;
}

export function supportedLangs(): Lang[] {
  return ["en", "fr"];
}
