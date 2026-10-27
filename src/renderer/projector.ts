// Projector window: resolution-independent fullscreen render of the current
// worship slide + alert overlay (bottom) + clock overlay (top-right).
// Slide HTML arrives pre-styled from the control window (header/body/footer
// templates, bilingual stacking, chord spans, background color/image/video-loop).
//
// Two things a worship room needs that a plain HTML dump does not give:
//
//   * the operator's point size is a *reference* size (the style's "48" is
//     written for a 1080p screen). On the projector it is scaled by the
//     screen height, so a verse covers the same share of a 720p screen as of
//     a 4K one instead of shrinking to a thumbnail on a big display;
//   * a verse that is too long for the frame is shrunk until it fits. The
//     frame clips overflow (`overflow:hidden`), so without this the bottom
//     of a long hymn verse would simply be invisible to the congregation.

/** Screen height the style's point size was authored against. */
const REFERENCE_HEIGHT = 1080;
/** Never shrink below this share of the scaled size: below it, the text is
 *  unreadable and the operator should split the verse instead. */
const MIN_SCALE = 0.45;

/** True when the slide body's content is taller than the frame it sits in. */
function bodyOverflows(body: HTMLElement): boolean {
  const frame = body.getBoundingClientRect();
  if (frame.height <= 0) return false;
  let top = Number.POSITIVE_INFINITY;
  let bottom = Number.NEGATIVE_INFINITY;
  for (const child of Array.from(body.children) as HTMLElement[]) {
    const rect = child.getBoundingClientRect();
    if (rect.width === 0 && rect.height === 0) continue;
    top = Math.min(top, rect.top);
    bottom = Math.max(bottom, rect.bottom);
  }
  if (!Number.isFinite(top)) return false;
  // Paragraph margins are not part of a child rect; a couple of pixels of
  // slack keeps a snug fit from triggering another shrink pass.
  return bottom - top > frame.height + 2;
}

/** Scale the slide to the projector and shrink it until the text fits. */
function fitSlide(slide: HTMLElement): { shrunk: boolean; factor: number } {
  // The authored size is remembered separately: the inline font-size holds
  // the *result* of the last fit, and re-fitting after a resize must start
  // from the operator's size again instead of scaling the scaled value.
  const authored = parseFloat(slide.dataset["vpAuthored"] ?? slide.style.fontSize);
  if (!Number.isFinite(authored) || authored <= 0) return { shrunk: false, factor: 1 };
  slide.dataset["vpAuthored"] = String(authored);
  const scaled = authored * (window.innerHeight / REFERENCE_HEIGHT);
  let size = scaled;
  slide.style.fontSize = `${size.toFixed(2)}px`;
  const body = slide.querySelector<HTMLElement>(".vp-body");
  if (!body) return { shrunk: false, factor: 1 };
  const floor = size * MIN_SCALE;
  for (let pass = 0; pass < 40 && size > floor; pass++) {
    if (!bodyOverflows(body)) break;
    size = Math.max(floor, size * 0.94);
    slide.style.fontSize = `${size.toFixed(2)}px`;
  }
  const factor = scaled > 0 ? size / scaled : 1;
  return { shrunk: factor < 0.995, factor };
}

function fitAllSlides(root: ParentNode): { shrunk: boolean; factor: number } {
  let shrunk = false;
  let factor = 1;
  root.querySelectorAll<HTMLElement>(".vp-slide").forEach((slide) => {
    const result = fitSlide(slide);
    if (result.shrunk) {
      shrunk = true;
      factor = Math.min(factor, result.factor);
    }
  });
  return { shrunk, factor };
}

function render(state: { slide: { title: string; html: string }; alert: { visible: boolean; text: string } }): void {
  const slide = document.getElementById("slide")!;
  slide.innerHTML = state.slide.html || `<h1>VideoPsalm</h1><p>Nothing projected.</p>`;
  // Full-bleed worship slides carry their own background: drop the vmin padding
  // so color/image/video covers the whole screen; plain slides keep padding.
  slide.classList.toggle("bleed", slide.querySelector(".vp-slide") !== null);
  // Background color carried on the wrapper: mirror it onto the stage so
  // letterboxing never shows a mismatched black bar.
  const wrap = slide.querySelector(".vp-slide") as HTMLElement | null;
  if (wrap) {
    const bg = wrap.style.backgroundColor;
    if (bg) (slide as HTMLElement).style.background = bg;
    else (slide as HTMLElement).style.background = "#000";
  } else {
    (slide as HTMLElement).style.background = "#000";
  }
  // Ensure looping background videos actually play (autoplay attribute + nudge).
  slide.querySelectorAll("video[autoplay]").forEach((v) => {
    const vid = v as HTMLVideoElement;
    void vid.play().catch(() => undefined);
  });
  // Size the lyrics for this screen, and never let a long verse be clipped.
  const fit = fitAllSlides(slide);
  // Tell the control window when the text had to be shrunk, so the operator
  // learns that the verse is too long for the frame *before* the service.
  window.vp.reportFit({ shrunk: fit.shrunk, factor: Math.round(fit.factor * 100) / 100 });
  const overlay = document.getElementById("alert-overlay")!;
  overlay.hidden = !state.alert.visible;
  overlay.textContent = state.alert.text;
}

window.vp.onRendered((data) => render(data as { slide: { title: string; html: string }; alert: { visible: boolean; text: string } }));
window.vp.onClock((text) => {
  document.getElementById("clock-overlay")!.textContent = text;
});

// A projector that is resized (or moved to another screen) has to re-fit the
// text, otherwise the verse that fitted before is clipped now.
window.addEventListener("resize", () => {
  const slide = document.getElementById("slide");
  if (slide) fitAllSlides(slide);
});

// Initial state (in case projector opened after a slide was projected).
void (window.vp.getProjectorState() as Promise<{ slide: { title: string; html: string }; alert: { visible: boolean; text: string } }>).then(render);

export {};
