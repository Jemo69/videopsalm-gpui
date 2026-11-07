// Agenda serde: create / serialize / parse portable agenda files.
// SPEC section 4 shape: {version:1, title, items:[...], settings}.

import { AGENDA_VERSION, isAgendaItemKind, type Agenda, type AgendaItem, type AgendaItemKind } from "./schemas";

let counter = 0;
export function newId(prefix = "item"): string {
  counter += 1;
  return `${prefix}-${Date.now().toString(36)}-${counter.toString(36)}`;
}

export function createAgenda(title: string): Agenda {
  return { version: 1, title, items: [], settings: {} };
}

export function addItem(
  agenda: Agenda,
  kind: AgendaItemKind,
  title: string,
  ref?: AgendaItem["ref"],
  extra?: Partial<AgendaItem>
): AgendaItem {
  if (!isAgendaItemKind(kind)) throw new Error(`unknown agenda kind: ${kind}`);
  const item: AgendaItem = { id: newId(kind.toLowerCase()), kind, title, ref, ...extra };
  agenda.items.push(item);
  return item;
}

export function moveItem(agenda: Agenda, id: string, dir: -1 | 1): boolean {
  const i = agenda.items.findIndex((x) => x.id === id);
  const j = i + dir;
  if (i < 0 || j < 0 || j >= agenda.items.length) return false;
  [agenda.items[i], agenda.items[j]] = [agenda.items[j], agenda.items[i]];
  return true;
}

export function removeItem(agenda: Agenda, id: string): boolean {
  const i = agenda.items.findIndex((x) => x.id === id);
  if (i < 0) return false;
  agenda.items.splice(i, 1);
  return true;
}

export function renameItem(agenda: Agenda, id: string, title: string): boolean {
  const it = agenda.items.find((x) => x.id === id);
  if (!it) return false;
  it.title = title;
  return true;
}

export function serializeAgenda(agenda: Agenda): string {
  return JSON.stringify({ version: 1, title: agenda.title, items: agenda.items, settings: agenda.settings ?? {} }, null, 2);
}

export function parseAgenda(json: string): Agenda {
  let raw: unknown;
  try {
    raw = JSON.parse(json);
  } catch {
    throw new Error("agenda file is not valid JSON");
  }
  if (typeof raw !== "object" || raw === null) throw new Error("agenda file must be an object");
  const o = raw as Record<string, unknown>;
  if (o["version"] !== AGENDA_VERSION) throw new Error(`unsupported agenda version: ${String(o["version"])}`);
  if (typeof o["title"] !== "string") throw new Error("agenda title must be a string");
  if (!Array.isArray(o["items"])) throw new Error("agenda items must be an array");
  const items: AgendaItem[] = o["items"].map((it: unknown, idx: number) => {
    if (typeof it !== "object" || it === null) throw new Error(`agenda item ${idx} must be an object`);
    const r = it as Record<string, unknown>;
    if (typeof r["id"] !== "string" || !r["id"]) throw new Error(`agenda item ${idx} missing id`);
    if (typeof r["kind"] !== "string" || !isAgendaItemKind(r["kind"])) {
      throw new Error(`agenda item ${idx} has unknown kind: ${String(r["kind"])}`);
    }
    if (typeof r["title"] !== "string") throw new Error(`agenda item ${idx} missing title`);
    return {
      id: r["id"] as string,
      kind: r["kind"] as AgendaItemKind,
      title: r["title"] as string,
      ref: (r["ref"] as AgendaItem["ref"]) ?? undefined,
      flowType: (r["flowType"] as AgendaItem["flowType"]) ?? undefined,
      autoAdvanceMs: (r["autoAdvanceMs"] as number) ?? undefined,
      overrides: (r["overrides"] as Record<string, unknown>) ?? undefined,
    };
  });
  return {
    version: 1,
    title: o["title"] as string,
    items,
    settings: (o["settings"] as Record<string, unknown>) ?? {},
  };
}
