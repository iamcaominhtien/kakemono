// The layout. The text below calls the functions `kakemono-…` for the
// parts that the layout decides on.

// Pages that are part of the front matter: up to the title page (and the
// cover).
#let kakemono-front(p) = query(<kakemono-front>).any(m => m.location().page() >= p)

// The blank page that a chapter leaves before the next one, which starts
// on a right-hand page: the end of a chapter (marked where the next one
// starts) is before page `p` and its next chapter after it.
#let kakemono-blank(p, chapters) = {
  let ends = query(<kakemono-chapter-end>)
  chapters.enumerate().any(((i, h)) => (
    h.location().page() > p and i < ends.len() and ends.at(i).location().page() < p
  ))
}

#let kakemono(body) = {
  let book = doc-layout == "book"
  set document(title: doc-title)
  set text(font: fonts, size: font-size, lang: lang)
  set par(justify: true, leading: 0.6em, spacing: 0.6em, first-line-indent: 1.2em)
  set heading(numbering: none)
  show heading: set block(above: 1.4em, below: 0.9em)
  show heading: it => { set par(justify: false); it }
  show list: set block(above: 0.6em, below: 0.6em)
  show math.equation.where(block: true): set block(above: 0.9em, below: 0.9em)
  show raw: set text(size: 0.9em)
  show footnote.entry: set par(first-line-indent: 0pt)
  let page-size = if type(paper) == str { (paper: paper) } else { paper }
  if not book {
    set page(..page-size, margin: 2.5cm, numbering: "1")
    body
    return
  }
  // A book: mirrored margins, the title of the book on the left-hand
  // pages and that of the chapter on the right-hand ones (with the page
  // number at the outside edge), none on the front matter, on the opening
  // page of a chapter (which has its number at the foot) and on the blank
  // pages.
  set page(
    ..page-size,
    margin: (inside: 13%, outside: 10%, top: 11%, bottom: 12%),
    numbering: "1",
    header: context {
      let p = here().page()
      let chapters = query(heading.where(level: 1))
      if kakemono-front(p) or chapters.any(h => h.location().page() == p) or kakemono-blank(p, chapters) {
        return
      }
      let number = counter(page).display()
      let before = chapters.filter(h => h.location().page() < p)
      set text(size: 0.85em)
      if calc.even(p) {
        [#number #h(1fr) #smallcaps(doc-title)]
      } else {
        [#if before.len() > 0 { emph(before.last().body) } #h(1fr) #number]
      }
    },
    footer: context {
      let p = here().page()
      let chapters = query(heading.where(level: 1))
      if not kakemono-front(p) and chapters.any(h => h.location().page() == p) {
        align(center, text(size: 0.85em, counter(page).display()))
      }
    },
  )
  // A chapter opens on a right-hand page, a third down, and numbers its
  // notes from 1.
  show heading.where(level: 1): set text(size: 1.45em, weight: "regular")
  show heading.where(level: 1): set align(center)
  show heading.where(level: 1): set block(above: 0pt, below: 2.2em)
  show heading.where(level: 1): it => {
    [#metadata(none) <kakemono-chapter-end>]
    pagebreak(weak: true, to: "odd")
    counter(footnote).update(0)
    v(18%)
    it
  }
  show heading.where(level: 2): set text(size: 1.1em)
  if cover != none {
    page(margin: 0pt, header: none, footer: none, image(cover, width: 100%, height: 100%, fit: "cover"))
    [#metadata(none) <kakemono-front>]
    pagebreak(to: "odd")
  }
  body
}

// The title: on a title page of its own in a book.
#let kakemono-title(body) = {
  if doc-layout == "book" {
    [#metadata(none) <kakemono-front>]
    v(22%)
    align(center, par(justify: false, text(size: 1.9em, body)))
    v(2em)
  } else {
    align(center, block(below: 1.2em, par(justify: false, text(size: 1.7em, weight: "bold", body))))
  }
}

// Author groups that stand side by side (each one a name and its lines).
#let kakemono-authors(..cards) = align(center, par(justify: false, cards.pos().map(card => box(align(center, card))).join(h(2em))))

// A group of authors too long to stand beside another.
#let kakemono-byline(body) = align(center, par(justify: false, body))

// A link to where the document comes from (its arXiv page).
#let kakemono-source(body) = align(center, block(above: 1em, below: 1.2em, body))

// The abstract, with its heading.
#let kakemono-abstract(title, body) = pad(x: 2em, {
  set text(size: 0.92em)
  align(center, strong(title))
  body
})

// The opening words of a chapter, in small capitals in a book (Vietnamese
// letters lack small capitals in most fonts).
#let kakemono-opening(body) = if doc-layout == "book" and lang != "vi" { smallcaps(body) } else { body }

// A scene break of a novel.
#let kakemono-break() = align(center, block(above: 1.2em, below: 1.2em)[\* #h(0.8em) \* #h(0.8em) \*])

// A link to a place of the document; the text alone when the place is not
// in it.
#let kakemono-link(target, body) = context {
  let found = query(target)
  if found.len() > 0 { link(found.first().location(), body) } else { body }
}

// A list item with the marker of the document, at its nesting level.
#let kakemono-item(level, marker, body) = pad(left: level * 1.5em, list(marker: marker, body))

// A paragraph that continues a list item.
#let kakemono-continuation(level, body) = pad(left: level * 1.5em + 1.2em, body)

// An entry of the bibliography.
#let kakemono-reference(body) = par(first-line-indent: 0pt, hanging-indent: 1.5em, text(size: 0.9em, body))

// A footnote of the document, where it was, with its mark.
#let kakemono-note(marker, body) = par(first-line-indent: 0pt, text(size: 0.85em)[#super(marker)#h(0.2em)#body])

// A note without a mark (in the margin of a book).
#let kakemono-aside(body) = pad(x: 2em, text(size: 0.9em, body))

// The table of contents, with the page numbers of this layout.
#let kakemono-contents() = outline(title: none, depth: 2)

// Code.
#let kakemono-code(code) = block(width: 100%, raw(block: true, code))

// A displayed formula with its number, at the right.
#let kakemono-equation(number, body) = if number == none { body } else {
  block(width: 100%, breakable: false, {
    body
    place(right + horizon, [(#number)])
  })
}

// A picture of an area of the page: as wide as it was, at most as wide as
// the text.
#let kakemono-picture(path, width) = align(center, layout(size => image(path, width: calc.min(width, size.width))))

// A figure or a table and its caption, kept on one page.
#let kakemono-figure(body) = block(width: 100%, breakable: false, above: 1em, below: 1em, body)

// A caption, with its label ("Figure 3:") in bold.
#let kakemono-caption(label, body) = par(first-line-indent: 0pt, text(size: 0.9em)[#if label != none [#strong(label) ]#body])

// A table, with rules above, below and under its header.
#let kakemono-table(..args) = {
  set text(size: 0.9em)
  set par(justify: false, first-line-indent: 0pt)
  align(center, table(stroke: none, inset: (x: 0.4em, y: 0.3em), ..args))
}
