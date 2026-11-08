R++ WRITE 2.1
=============

A word processor for writing books, for TinyDOS on rusty486. It is written
in R++ (a subset of Rust) and built with Rustland R++; the source is
C:\RPP\WRITE.RS.

START
  WRITE              start with the welcome document
  WRITE SAMPLE       open SAMPLE.WP, a short sample book
  WRITE BOOK.TXT     import a plain text or Markdown file

MENUS
  F10, or Alt + the highlighted letter: File, Edit, Search, Format, Style,
  View, Book, Options, Help. Arrows move, Enter chooses, Esc closes.

FILES (File menu)
  New, Open (with a list of .WP and .TXT files), Save, Save as, Export
  text, Rename file, Delete file, Exit. Documents are saved as .WP files,
  which keep all formatting. Export writes plain text with Markdown-style
  headings (# chapter, ## section, > quote), which Open reads back.
  R++ Write asks before discarding unsaved changes.

KEYS
  Ctrl+N new        Ctrl+O open       Ctrl+S save       Alt+X exit
  Shift+arrows select               Ctrl+X/C/V cut, copy, paste
  Ctrl+Z undo       Ctrl+A select all Ctrl+Y delete paragraph
  Ctrl+F find       F3 find next      Ctrl+H replace all
  Ctrl+G go to page Ctrl+Home/End     Ctrl+PgUp/PgDn previous/next chapter
  Ctrl+B bold       Ctrl+I italic     Ctrl+U underline  Ctrl+P plain
  Ctrl+T strikethrough                Ctrl+K highlight
  Ctrl+L left       Ctrl+E centre     Ctrl+R right      Ctrl+J justify
  Alt+1 normal      Alt+2 chapter     Alt+3 section
  Alt+4 block quote Alt+5 scene break Alt+6 writer's note
  Alt+7 bullet list Alt+8 numbered list
  F1 keys           F5 outline        F7 focus mode     F8 settings
  F9 statistics     Format > UPPERCASE / lowercase / Title Case

  Style keys work on the selection, or switch the style of new text.

BOOK WRITING
  Chapter and section headings make up the outline (F5), which lists the
  words and page of each chapter and jumps to it. Writer's notes stay in the
  document but out of the word count and the export. Pages are marked as
  you write (View > Page breaks; Options > Page length). The status bar
  shows the page, line, word count and progress towards a session word
  goal (Book > Word goal). Focus mode (F7) hides everything but the text;
  typewriter scrolling keeps the current line in the middle of the screen.

SETTINGS (F8, or Options > Settings)
  First-line indent on/off and its size, space between paragraphs, page
  breaks and page length, typewriter scrolling, auto-save (every 300 edits
  once a document has a name), the session word goal and the colours
  (Borland Blue, Paper, Amber, Midnight). Changes apply at once and are
  saved in WRITE.CFG in the current directory.

LIMITS
  2,000,000 characters per document (about 330,000 words). At 95% the
  status bar warns you; when the document is full, R++ Write offers to save
  it and continue in the next file of a numbered sequence (BOOK.WP,
  BOOK2.WP, BOOK3.WP...). The new file starts with a writer's note saying
  which file it continues, and the session word goal carries over.
  Undo remembers about a megabyte of changes (hundreds of steps).
  Opening a .TXT file imports it; it is saved as a .WP file next to it.
