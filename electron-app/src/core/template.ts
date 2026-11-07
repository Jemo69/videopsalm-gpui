/**
 * Header and footer template engine (B-21).
 *
 * Evaluates slide header and footer format strings. Supports both the original
 * VideoPsalm `[Token]` syntax (e.g. `[SongTitle]`, `[SongVerseRank]`, `[SongBookTitle]`,
 * `[BibleBookName]`, etc.) and modern `{token}` aliases (e.g. `{title}`, `{ref}`,
 * `{position 3/5}`, `{position}`, `{verse_tag}`, etc.).
 */

export interface SlideTemplateContext {
  title?: string;
  author?: string;
  composer?: string;
  copyright?: string;
  ccli?: string;
  songbook?: string;
  songbookAbbrev?: string;
  verseRank?: number; // 1-based index
  verseCount?: number; // total visible verses
  verseTag?: string;
  bibleBook?: string;
  bibleChapter?: number;
  bibleVerse?: string | number;
  bibleName?: string;
  reference?: string;
  license?: string;
}

export function formatTemplate(template: string, ctx: SlideTemplateContext): string {
  if (!template) return "";

  let out = template;

  const rankStr = ctx.verseRank != null ? String(ctx.verseRank) : "";
  const countStr = ctx.verseCount != null ? String(ctx.verseCount) : "";
  const posStr =
    ctx.verseRank != null && ctx.verseCount != null
      ? `${ctx.verseRank}/${ctx.verseCount}`
      : rankStr;
  const versePosStr =
    ctx.verseRank != null && ctx.verseCount != null
      ? `verse ${ctx.verseRank}/${ctx.verseCount}`
      : ctx.verseRank != null
      ? `verse ${ctx.verseRank}`
      : "";

  // VideoPsalm legacy tokens:
  out = out.split("[SongTitle]").join(ctx.title ?? "");
  out = out.split("[SongAuthor]").join(ctx.author ?? "");
  out = out.split("[SongComposer]").join(ctx.composer ?? "");
  out = out.split("[SongCopyright]").join(ctx.copyright ?? "");
  out = out.split("[SongCCLI]").join(ctx.ccli ?? "");
  out = out.split("[SongBookTitle]").join(ctx.songbook ?? "");
  out = out.split("[SongBookAbbreviation]").join(ctx.songbookAbbrev ?? "");
  out = out.split("[SongVerseRank]").join(rankStr);
  out = out.split("[SongVerseCount]").join(countStr);
  out = out.split("[SongVerseTag]").join(ctx.verseTag ?? "");
  out = out.split("[SiteLicense]").join(ctx.license ?? "");

  out = out.split("[BibleBookName]").join(ctx.bibleBook ?? "");
  const chapStr = ctx.bibleChapter != null ? String(ctx.bibleChapter) : "";
  out = out.split("[BibleChapterID]").join(chapStr);
  const vStr = ctx.bibleVerse != null ? String(ctx.bibleVerse) : "";
  out = out.split("[BibleVerseID]").join(vStr);
  out = out.split("[BibleName]").join(ctx.bibleName ?? "");
  out = out.split("[BibleCopyright]").join(ctx.copyright ?? "");

  // Curly bracket aliases (B-21 & UX-RESEARCH §5.5):
  out = out.split("{position 3/5}").join(posStr);
  out = out.split("{position}").join(posStr);
  out = out.split("{verse position}").join(versePosStr);
  out = out.split("{verse_position}").join(versePosStr);
  out = out.split("{title}").join(ctx.title ?? "");
  out = out.split("{author}").join(ctx.author ?? "");
  out = out.split("{composer}").join(ctx.composer ?? "");
  out = out.split("{ccli}").join(ctx.ccli ?? "");
  out = out.split("{copyright}").join(ctx.copyright ?? "");
  out = out.split("{songbook}").join(ctx.songbook ?? "");
  out = out.split("{songbook_code}").join(ctx.songbookAbbrev ?? "");
  out = out.split("{songbook_abbrev}").join(ctx.songbookAbbrev ?? "");
  out = out.split("{verse_num}").join(rankStr);
  out = out.split("{verse_count}").join(countStr);
  out = out.split("{verse_tag}").join(ctx.verseTag ?? "");
  out = out.split("{tag}").join(ctx.verseTag ?? "");
  out = out.split("{book}").join(ctx.bibleBook ?? "");
  out = out.split("{chapter}").join(chapStr);
  out = out.split("{verse}").join(vStr);
  out = out.split("{bible}").join(ctx.bibleName ?? "");
  out = out.split("{license}").join(ctx.license ?? "");
  out = out.split("{ref}").join(ctx.reference ?? "");
  out = out.split("{reference}").join(ctx.reference ?? "");

  return out;
}
