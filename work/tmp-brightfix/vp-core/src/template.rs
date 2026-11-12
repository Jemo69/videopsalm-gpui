//! Header and footer template engine (B-21).
//!
//! Evaluates slide header and footer format strings. Supports both the original
//! VideoPsalm `[Token]` syntax (e.g. `[SongTitle]`, `[SongVerseRank]`, `[SongBookTitle]`,
//! `[BibleBookName]`, etc.) and modern `{token}` aliases (e.g. `{title}`, `{ref}`,
//! `{position 3/5}`, `{position}`, `{verse_tag}`, etc.).

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SlideTemplateContext<'a> {
    pub title: Option<&'a str>,
    pub author: Option<&'a str>,
    pub composer: Option<&'a str>,
    pub copyright: Option<&'a str>,
    pub ccli: Option<&'a str>,
    pub songbook: Option<&'a str>,
    pub songbook_abbrev: Option<&'a str>,
    pub verse_rank: Option<usize>,
    pub verse_count: Option<usize>,
    pub verse_tag: Option<&'a str>,
    pub bible_book: Option<&'a str>,
    pub bible_chapter: Option<u32>,
    pub bible_verse: Option<&'a str>,
    pub bible_name: Option<&'a str>,
    pub reference: Option<&'a str>,
    pub license: Option<&'a str>,
}

impl<'a> SlideTemplateContext<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    pub fn with_verse(mut self, rank: usize, count: usize, tag: Option<&'a str>) -> Self {
        self.verse_rank = Some(rank);
        self.verse_count = Some(count);
        self.verse_tag = tag;
        self
    }

    pub fn with_song(
        mut self,
        title: &'a str,
        author: Option<&'a str>,
        songbook: Option<&'a str>,
        songbook_abbrev: Option<&'a str>,
    ) -> Self {
        self.title = Some(title);
        self.author = author;
        self.songbook = songbook;
        self.songbook_abbrev = songbook_abbrev;
        self
    }

    pub fn with_bible(
        mut self,
        book: &'a str,
        chapter: u32,
        verse: &'a str,
        bible_name: Option<&'a str>,
        full_ref: &'a str,
    ) -> Self {
        self.bible_book = Some(book);
        self.bible_chapter = Some(chapter);
        self.bible_verse = Some(verse);
        self.bible_name = bible_name;
        self.reference = Some(full_ref);
        self
    }
}

/// Formats a template string replacing all tokens using the provided context.
pub fn format_template(template: &str, ctx: &SlideTemplateContext) -> String {
    if template.is_empty() {
        return String::new();
    }

    let mut out = template.to_string();

    let rank_str = ctx.verse_rank.map(|r| r.to_string()).unwrap_or_default();
    let count_str = ctx.verse_count.map(|c| c.to_string()).unwrap_or_default();
    let pos_str = match (ctx.verse_rank, ctx.verse_count) {
        (Some(r), Some(c)) => format!("{r}/{c}"),
        (Some(r), None) => r.to_string(),
        _ => String::new(),
    };
    let verse_pos_str = match (ctx.verse_rank, ctx.verse_count) {
        (Some(r), Some(c)) => format!("verse {r}/{c}"),
        (Some(r), None) => format!("verse {r}"),
        _ => String::new(),
    };

    // VideoPsalm legacy tokens:
    out = out.replace("[SongTitle]", ctx.title.unwrap_or_default());
    out = out.replace("[SongAuthor]", ctx.author.unwrap_or_default());
    out = out.replace("[SongComposer]", ctx.composer.unwrap_or_default());
    out = out.replace("[SongCopyright]", ctx.copyright.unwrap_or_default());
    out = out.replace("[SongCCLI]", ctx.ccli.unwrap_or_default());
    out = out.replace("[SongBookTitle]", ctx.songbook.unwrap_or_default());
    out = out.replace(
        "[SongBookAbbreviation]",
        ctx.songbook_abbrev.unwrap_or_default(),
    );
    out = out.replace("[SongVerseRank]", &rank_str);
    out = out.replace("[SongVerseCount]", &count_str);
    out = out.replace("[SongVerseTag]", ctx.verse_tag.unwrap_or_default());
    out = out.replace("[SiteLicense]", ctx.license.unwrap_or_default());

    out = out.replace("[BibleBookName]", ctx.bible_book.unwrap_or_default());
    let chap_str = ctx.bible_chapter.map(|c| c.to_string()).unwrap_or_default();
    out = out.replace("[BibleChapterID]", &chap_str);
    out = out.replace("[BibleVerseID]", ctx.bible_verse.unwrap_or_default());
    out = out.replace("[BibleName]", ctx.bible_name.unwrap_or_default());
    out = out.replace("[BibleCopyright]", ctx.copyright.unwrap_or_default());

    // Curly bracket aliases (B-21 & UX-RESEARCH §5.5):
    out = out.replace("{position 3/5}", &pos_str);
    out = out.replace("{position}", &pos_str);
    out = out.replace("{verse position}", &verse_pos_str);
    out = out.replace("{verse_position}", &verse_pos_str);
    out = out.replace("{title}", ctx.title.unwrap_or_default());
    out = out.replace("{author}", ctx.author.unwrap_or_default());
    out = out.replace("{composer}", ctx.composer.unwrap_or_default());
    out = out.replace("{ccli}", ctx.ccli.unwrap_or_default());
    out = out.replace("{copyright}", ctx.copyright.unwrap_or_default());
    out = out.replace("{songbook}", ctx.songbook.unwrap_or_default());
    out = out.replace("{songbook_code}", ctx.songbook_abbrev.unwrap_or_default());
    out = out.replace("{songbook_abbrev}", ctx.songbook_abbrev.unwrap_or_default());
    out = out.replace("{verse_num}", &rank_str);
    out = out.replace("{verse_count}", &count_str);
    out = out.replace("{verse_tag}", ctx.verse_tag.unwrap_or_default());
    out = out.replace("{tag}", ctx.verse_tag.unwrap_or_default());
    out = out.replace("{book}", ctx.bible_book.unwrap_or_default());
    out = out.replace("{chapter}", &chap_str);
    out = out.replace("{verse}", ctx.bible_verse.unwrap_or_default());
    out = out.replace("{bible}", ctx.bible_name.unwrap_or_default());
    out = out.replace("{license}", ctx.license.unwrap_or_default());
    out = out.replace("{ref}", ctx.reference.unwrap_or_default());
    out = out.replace("{reference}", ctx.reference.unwrap_or_default());

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_song_tokens_expansion() {
        let ctx = SlideTemplateContext {
            title: Some("Amazing Grace"),
            author: Some("John Newton"),
            songbook: Some("Hymns"),
            songbook_abbrev: Some("HYM"),
            verse_rank: Some(3),
            verse_count: Some(5),
            verse_tag: Some("Verse 3"),
            ccli: Some("12345"),
            ..Default::default()
        };

        // VideoPsalm original tokens
        let legacy = "[SongTitle] ([SongBookAbbreviation]) - [SongVerseTag] ([SongVerseRank]/[SongVerseCount])";
        assert_eq!(
            format_template(legacy, &ctx),
            "Amazing Grace (HYM) - Verse 3 (3/5)"
        );

        // Curly aliases
        let modern = "{title} — {position 3/5} | {author}";
        assert_eq!(
            format_template(modern, &ctx),
            "Amazing Grace — 3/5 | John Newton"
        );

        let pos = "{position}";
        assert_eq!(format_template(pos, &ctx), "3/5");

        let verse_pos = "{verse position}";
        assert_eq!(format_template(verse_pos, &ctx), "verse 3/5");
    }

    #[test]
    fn test_bible_tokens_expansion() {
        let ctx = SlideTemplateContext {
            bible_book: Some("John"),
            bible_chapter: Some(3),
            bible_verse: Some("16"),
            bible_name: Some("KJV"),
            reference: Some("John 3:16"),
            ..Default::default()
        };

        let legacy = "[BibleBookName] [BibleChapterID]:[BibleVerseID] ([BibleName])";
        assert_eq!(format_template(legacy, &ctx), "John 3:16 (KJV)");

        let modern = "{ref} — {bible}";
        assert_eq!(format_template(modern, &ctx), "John 3:16 — KJV");
    }

    #[test]
    fn test_empty_or_missing_tokens() {
        let ctx = SlideTemplateContext::default();
        let tpl = "{title} {position 3/5}";
        assert_eq!(format_template(tpl, &ctx), " ");
    }
}
