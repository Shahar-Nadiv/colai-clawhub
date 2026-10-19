---
name: marked-screenshots
description: Read a screenshot the person has marked up — a circle, box, arrow, highlight, pin or freehand stroke drawn on it, including marks made with the Colai toolbar — and answer about exactly what is marked rather than the whole screen. Use whenever an image arrives with annotations on it, or the person says "this", "here", "the circled bit", "what I marked" or similar about a picture.
---

# Marked screenshots

When somebody draws on a screenshot, the drawing is the question. A circle around a button
means "this button"; an arrow means "the thing at its tip"; a highlight means "these words".
Answer about what is marked. The rest of the picture is context, not the subject.

## Find the marks

Look for anything drawn on top of the screen rather than part of it:

- **Circles and ellipses** — usually a bright colour, often hand-drawn and not quite closed.
- **Boxes and rectangles** — an outline around a region, sometimes with a thicker or dashed edge.
- **Arrows** — the tip is what they mean, not the tail.
- **Highlights** — a translucent band over text or an area.
- **Pins and dots** — a small marker at a single point. Colai draws a ring at the spot pointed at.
- **Freehand strokes** — underlines, ticks, scribbles across something to be removed.
- **Numbers or labels** beside marks — "1", "2", or a word — tie each mark to a step in the
  person's message.

Colai's marks arrive as their own pictures, cropped to what was marked, with a line in the
message for each — "Point at (mark-1.png) at 684,458", "Box (mark-2.png)", and so on — and the
words the person typed for that mark. Treat each line and its picture as one mark.

If something could be either a mark or part of the interface (a red outline on a form field
that might be a validation error), say which you took it as.

## Read what is marked

For each mark, before answering:

1. Say what it is on: the exact text, the control, the region, the value. Quote text exactly as
   it appears, including numbers, units and error codes.
2. Note what is just around it when that changes the meaning — the label of the field a value
   sits in, the heading of the section, the row and column of a table cell.
3. Match it to the person's words. With several marks and several requests, pair them in order
   or by their numbers, and say how you paired them if it isn't obvious.

If a mark's target can't be read — too small, blurred, cut off at the edge — say so plainly and
say what you can see, rather than guessing at the text.

## Answer

- Lead with the marked thing: "The button you circled, **Save draft**, is disabled because…"
- Keep to what was marked unless something elsewhere in the picture is the cause or the fix,
  and then say why you are pointing outside the mark.
- For a change to make (copy, colour, spacing, layout), give the specific change for the marked
  element: the new text, the value, where it moves.
- For a problem (an error message, a wrong number, a broken layout), say what it means, the
  likely cause visible in the picture, and what to do next.
- With several marks, answer them in order, one short part per mark.

Don't describe the whole screenshot back to the person, and don't ask them what they marked
when the mark is clear.
