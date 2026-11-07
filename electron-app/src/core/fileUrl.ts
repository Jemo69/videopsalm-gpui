// A `file://` URL that is safe to put inside an HTML attribute.
//
// The projected slide is built as an HTML *string* and handed to the
// projector with `innerHTML`. A raw `url("file:///…")` or `src="file:///…"`
// closes the attribute at the first inner quote, so the style/src never
// reaches the browser: the background image and every media item silently
// fail to show. Percent-encoding the few characters that break a URL (and
// the attribute) fixes it.
//
// `:` is deliberately left alone so Windows paths (`C:\Songs\…`) still work,
// and `\` is turned into `/` for the same reason.

export function fileUrl(file: string): string {
  const path = file.replace(/\\/g, "/");
  return `file://${path.replace(/[%#?&"\s]/g, (ch) => encodeURIComponent(ch))}`;
}
