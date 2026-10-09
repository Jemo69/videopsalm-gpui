/**
 * Style explorer tree view — **definition-of-done #6 UI**.
 *
 * "Open the style explorer and read **which level** a font size came from."
 *
 * The cascade MODEL already exists (`src/core/cascade.ts`): five chains from
 * `Base`, per-attribute provenance via `explorerRows(cascade, chain, atLevel)`.
 * This module is the visible tree on top of it. It never reimplements
 * resolution — every leaf comes out of `explorerRows`.
 *
 * Tree shape: five chains (`Songs`, `Bibles`, `Images`, `Videos`,
 * `PowerPoint`), each expandable to its levels (`Base → … → most-specific`),
 * each level expandable to its 11 attributes in the original's order. Every
 * leaf shows the EFFECTIVE value, the SOURCE level that set it, and an
 * inherited marker when the source is more general than the inspected level.
 * No cross-kind inheritance is shown: Bible levels never appear under Songs
 * (OBSERVATIONS.md §8 — each chain renders exactly its own `chainLevels`).
 *
 * Placement: this panel mounts inside the **Text ribbon tab's panel** (the
 * style editing surface), never floating over the projector. The canonical
 * 1.29 screenshot (`SCREENSHOT-1.29-LAYOUT.md`) pins the ribbon tabs and the
 * three columns but does not show the explorer open, so its exact dock within
 * the style surface is ambiguous — the Text tab's panel is the documented
 * fallback, not a guess presented as fact.
 *
 * Level selector: the original's five names, most-specific-first
 * (`Verse, Song, Songbook, Songbooks, Base` — `style-level-selector.png`,
 * `selectorCaption`). Media levels (`Images`, `Videos`, …) are NOT in the
 * selector: in the original they are reachable through the gallery path, so
 * the selector stays short and media levels are reached by expanding the
 * Images/Videos/PowerPoint chains directly.
 *
 * Reset: per-LEVEL reset via `resetLevel` (clears the whole local override so
 * every attribute re-inherits) AND per-ATTRIBUTE clear via `clearAttribute`
 * below (clears one leaf; cheap because a level is just a keyed object, and
 * `setLevel` drops the level entry when nothing is left). The tree repaints
 * after either, so a cleared leaf visibly flips back to its inherited source.
 *
 * Session state only: the cascade lives in `ExplorerState` in memory. It is
 * seeded once from the current prefs at `Base` (mirroring the Rust side's
 * `adopt_flat`), and it is deliberately NOT persisted to the agenda file —
 * persisting it would mean changing the shared `Style` schema, which is
 * owned by another workstream.
 */

import {
  Attribute,
  Cascade,
  Chain,
  CHAINS,
  ExplorerRow,
  Level,
  attributeSet,
  chainLevels,
  emptyCascade,
  explorerRows,
  getLevel,
  resetLevel,
  selectorCaption,
  setLevel,
} from "../core/cascade";

/** The level selector: the original's five names, most-specific-first. */
export const SELECTOR_LEVELS: readonly Level[] = [
  "verse",
  "song",
  "songbook",
  "songbooks",
  "base",
];

/** Captions for the selector, in order. */
export function selectorCaptions(): string[] {
  return SELECTOR_LEVELS.map(selectorCaption);
}

/** Chain labels in tree order. */
export const CHAIN_LABELS: Record<Chain, string> = {
  songs: "Songs",
  bibles: "Bibles",
  images: "Images",
  videos: "Videos",
  powerPoint: "PowerPoint",
};

/** The chain paths, base → most-specific (OBSERVATIONS.md §8). */
export const CHAIN_PATHS: Record<Chain, string> = {
  songs: "Base → Songbooks → Songbook → Song → Song verse",
  bibles: "Base → Bibles → Bible → Testament → Book → Chapter → Verse",
  images: "Base → Images → Image",
  videos: "Base → Videos → Video",
  powerPoint: "Base → PowerPoints → PowerPoint → Slides",
};

/** Display captions for tree levels (agree with `selectorCaption` on its five). */
export const LEVEL_CAPTIONS: Record<Level, string> = {
  base: "Base",
  songbooks: "Songbooks",
  songbook: "Songbook",
  song: "Song",
  songVerse: "Song verse",
  bibles: "Bibles",
  bible: "Bible",
  testament: "Testament",
  book: "Book",
  chapter: "Chapter",
  verse: "Verse",
  images: "Images",
  image: "Image",
  videos: "Videos",
  video: "Video",
  powerPoints: "PowerPoints",
  powerPoint: "PowerPoint",
  slides: "Slides",
};

/** One expanded level: its 11 leaves plus what it sets locally. */
export interface LevelNode {
  level: Level;
  caption: string;
  rows: ExplorerRow[];
  /** Attributes this level sets itself — everything else it inherits. */
  local: Attribute[];
}

/** One chain of the forest: its own levels only, never another kind's. */
export interface ChainNode {
  chain: Chain;
  label: string;
  path: string;
  levels: LevelNode[];
}

export function chainKey(chain: Chain): string {
  return `chain:${chain}`;
}

export function levelKey(chain: Chain, level: Level): string {
  return `level:${chain}:${level}`;
}

/**
 * The whole forest for a cascade. Each level node inspects ITSELF (`atLevel`
 * is that level), so every leaf's `inherited` flag answers "inherited
 * relative to this level" straight from the model.
 */
export function buildExplorerTree(c: Cascade): ChainNode[] {
  return CHAINS.map((ch) => ({
    chain: ch,
    label: CHAIN_LABELS[ch],
    path: CHAIN_PATHS[ch],
    levels: chainLevels(ch).map((lv) => ({
      level: lv,
      caption: LEVEL_CAPTIONS[lv],
      rows: explorerRows(c, ch, lv),
      local: attributeSet(c, lv),
    })),
  }));
}

/**
 * Per-attribute reset: clear ONE leaf at `level` so it re-inherits, leaving
 * the level's other local values alone. Whole-level reset is `resetLevel`.
 */
export function clearAttribute(
  c: Cascade,
  level: Level,
  attr: Attribute,
): Cascade {
  const cur = getLevel(c, level);
  if (!cur || cur[attr] === undefined) return c;
  const next = { ...cur } as Record<string, unknown>;
  delete next[attr];
  return setLevel(c, level, next as Parameters<typeof setLevel>[2]);
}

/**
 * Seed the session cascade from the current prefs, adopted at `Base`
 * ("always define Base first" — UX-RESEARCH §5.1). Merges: prefs only fill
 * attributes `Base` does not already set.
 */
export function seedFromPrefs(
  c: Cascade,
  prefs: { font?: string; size?: number; color?: string },
): Cascade {
  const cur = getLevel(c, "base") ?? {};
  const next = { ...cur };
  if (prefs.font && next.font === undefined) next.font = prefs.font;
  if (
    prefs.size !== undefined &&
    Number.isFinite(prefs.size) &&
    next.size === undefined
  ) {
    next.size = prefs.size;
  }
  if (prefs.color && next.color === undefined) next.color = prefs.color;
  if (Object.keys(next).length === 0) return c;
  return setLevel(c, "base", next);
}

/**
 * Map the legacy `styleLevel` pref to a cascade level. The pref predates the
 * cascade and says `"verse"` where the cascade distinguishes the song verse
 * (`songVerse`, the original's "Strophe") from the Bible `verse` — here
 * `"verse"` means the song chain's most specific rung. Anything unknown falls
 * back to `"song"`.
 */
export function inspectedFromStyleLevel(s: string): Level {
  switch (s) {
    case "base":
      return "base";
    case "songbooks":
      return "songbooks";
    case "songbook":
      return "songbook";
    case "song":
      return "song";
    case "verse":
      return "songVerse";
    default:
      return "song";
  }
}

/** Live explorer state (session only — see the module header). */
export interface ExplorerState {
  cascade: Cascade;
  /** The selector's level: highlighted across chains, `✓` in the selector. */
  inspected: Level;
  /** Expanded node keys (`chain:…` / `level:…:…`). Chains open by default. */
  expanded: Set<string>;
}

export function createExplorerState(): ExplorerState {
  return {
    cascade: emptyCascade(),
    inspected: "song",
    expanded: new Set(CHAINS.map(chainKey)),
  };
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/**
 * String rendering of the tree (used by the DOM mount AND the headless
 * tests — one renderer, no DOM needed to verify rows/sources/flags).
 */
export function renderExplorerHtml(
  tree: ChainNode[],
  inspected: Level,
  expanded?: ReadonlySet<string>,
): string {
  const sel = SELECTOR_LEVELS.map(
    (lv) =>
      `<button type="button" data-action="select" data-level="${lv}" aria-pressed="${lv === inspected}" title="Inspect ${escapeHtml(LEVEL_CAPTIONS[lv])}">${lv === inspected ? "✓ " : ""}${escapeHtml(selectorCaption(lv))}</button>`,
  ).join("");
  const chains = tree.map((ch) => chainHtml(ch, inspected, expanded)).join("");
  return `<div class="se-selector" role="group" aria-label="Style level selector (songs and Bibles; media levels via the chains below)">${sel}</div><ul class="se-chains">${chains}</ul>`;
}

function chainHtml(ch: ChainNode, inspected: Level, expanded?: ReadonlySet<string>): string {
  const chainOpen = !expanded || expanded.has(chainKey(ch.chain));
  const levels = ch.levels
    .map((ln) => {
      const leaves = ln.rows
        .map((r) => {
          const marker =
            r.source === undefined
              ? `<span class="se-src">default</span>`
              : r.inherited
                ? `<span class="se-src">◉ ${escapeHtml(LEVEL_CAPTIONS[r.source])}</span> <span class="se-inherited" title="Inherited from a more general level">inherited</span>`
                : `<span class="se-src">◉ ${escapeHtml(LEVEL_CAPTIONS[r.source])}</span> <span class="se-local">set here</span>`;
          return `<li class="se-leaf" data-attr="${r.attr}" data-source="${r.source ?? ""}" data-inherited="${r.inherited}"><span class="se-label">${escapeHtml(r.label)}</span><span class="se-val">${escapeHtml(r.value)}</span>${marker}<button type="button" class="se-mini" data-action="clear-attr" data-level="${ln.level}" data-attr="${r.attr}" title="Clear ${escapeHtml(r.label)} at ${escapeHtml(ln.caption)} so it re-inherits">clear</button></li>`;
        })
        .join("");
      const current =
        ln.level === inspected
          ? ` <span class="se-current" title="The selector's level">✓ current</span>`
          : "";
      const levelOpen = !expanded || expanded.has(levelKey(ch.chain, ln.level));
      return `<li class="se-level" data-level="${ln.level}"><span data-action="toggle-level" data-chain="${ch.chain}" data-level="${ln.level}" role="button" tabindex="0">${levelOpen ? "⊟" : "⊞"} ${escapeHtml(ln.caption)} (${ln.local.length} set here)${current}</span> <button type="button" class="se-mini" data-action="reset-level" data-level="${ln.level}" title="Reset ${escapeHtml(ln.caption)}: clear the local override so every attribute re-inherits">reset</button><ul>${leaves}</ul></li>`;
    })
    .join("");
  return `<li class="se-chain" data-chain="${ch.chain}"><span data-action="toggle-chain" data-chain="${ch.chain}" role="button" tabindex="0">${chainOpen ? "⊟" : "⊞"} ${escapeHtml(ch.label)} <span class="se-path">${escapeHtml(ch.path)}</span></span><ul>${levels}</ul></li>`;
}

/**
 * Mount the explorer into `root` and repaint on every interaction.
 * Expansion is tracked by key; collapse hides the child `<ul>` via `hidden`.
 * Returns a repaint function so the host (renderer.ts) can refresh the tree
 * when outside state (e.g. the Styles `Level` dropdown) changes.
 */
export function mountStyleExplorer(
  root: HTMLElement,
  state: ExplorerState,
): () => void {
  function repaint(): void {
    const tree = buildExplorerTree(state.cascade);
    root.innerHTML = renderExplorerHtml(tree, state.inspected, state.expanded);
    // Apply expansion: chains default open, levels default shut.
    for (const ch of tree) {
      const chainLi = root.querySelector(
        `li.se-chain[data-chain="${ch.chain}"]`,
      );
      const chainUl = chainLi?.querySelector(":scope > ul");
      if (chainUl) {
        (chainUl as HTMLElement).hidden = !state.expanded.has(
          chainKey(ch.chain),
        );
      }
      for (const ln of ch.levels) {
        const levelLi = chainLi?.querySelector(
          `:scope > ul > li.se-level[data-level="${ln.level}"]`,
        );
        const levelUl = levelLi?.querySelector(":scope > ul");
        if (levelUl) {
          (levelUl as HTMLElement).hidden = !state.expanded.has(
            levelKey(ch.chain, ln.level),
          );
        }
      }
    }
  }
  root.addEventListener("click", (e) => {
    const t = (e.target as HTMLElement).closest(
      "[data-action]",
    ) as HTMLElement | null;
    if (!t || !root.contains(t)) return;
    const action = t.dataset["action"];
    if (action === "select" && t.dataset["level"]) {
      state.inspected = t.dataset["level"] as Level;
      // Selecting a level opens the chain(s) holding it, so the ✓ is visible.
      for (const ch of CHAINS) {
        if (chainLevels(ch).includes(state.inspected)) {
          state.expanded.add(chainKey(ch));
        }
      }
      repaint();
    } else if (action === "toggle-chain" && t.dataset["chain"]) {
      const k = chainKey(t.dataset["chain"] as Chain);
      if (state.expanded.has(k)) state.expanded.delete(k);
      else state.expanded.add(k);
      repaint();
    } else if (
      action === "toggle-level" &&
      t.dataset["chain"] &&
      t.dataset["level"]
    ) {
      const k = levelKey(
        t.dataset["chain"] as Chain,
        t.dataset["level"] as Level,
      );
      if (state.expanded.has(k)) state.expanded.delete(k);
      else state.expanded.add(k);
      repaint();
    } else if (action === "reset-level" && t.dataset["level"]) {
      state.cascade = resetLevel(state.cascade, t.dataset["level"] as Level);
      repaint();
    } else if (
      action === "clear-attr" &&
      t.dataset["level"] &&
      t.dataset["attr"]
    ) {
      state.cascade = clearAttribute(
        state.cascade,
        t.dataset["level"] as Level,
        t.dataset["attr"] as Attribute,
      );
      repaint();
    }
  });
  repaint();
  return repaint;
}
