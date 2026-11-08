"""Writes dos/write/SAMPLE.WP, a short sample book for R++ Write.

Format: "RWP2", the text length (3 bytes little-endian + a zero), the text
(paragraphs end in "\\n"), then one style byte per character. For ordinary
characters the style is 1 bold | 2 italic | 4 underline; for a paragraph's
"\\n" it is the paragraph kind (0 normal, 1 chapter, 2 section, 3 quote,
4 scene break, 5 note) | alignment << 4 (0 left, 1 centre, 2 right,
3 justify).
"""
import os

NORMAL, CHAPTER, SECTION, QUOTE, SCENE, NOTE = range(6)
LEFT, CENTER, RIGHT, JUSTIFY = range(4)

text = bytearray()
style = bytearray()


def para(kind, align, *runs):
    """runs: plain strings, or (string, char_style) pairs."""
    for r in runs:
        s, st = (r, 0) if isinstance(r, str) else r
        text.extend(s.encode("ascii"))
        style.extend([st] * len(s))
    text.append(10)
    style.append(kind | align << 4)


para(CHAPTER, CENTER, "The Lighthouse Keeper")
para(NORMAL, CENTER, ("a sample book for R++ Write", 2))
para(NORMAL, LEFT)
para(CHAPTER, CENTER, "Chapter One")
para(NORMAL, JUSTIFY, "The lighthouse stood at the end of the world, or so it seemed to Mara on "
     "the morning she arrived. The ferry had left her on the jetty with two suitcases and a "
     "letter of appointment, and then turned back into the fog as if it were glad to be gone.")
para(NORMAL, JUSTIFY, "She climbed the hundred and twelve steps to the lamp room before she even "
     "unpacked. The great lens caught the grey light and broke it into a hundred ", ("small", 2),
     " rainbows, and for the first time in months she felt she was somewhere ", ("on purpose", 1), ".")
para(NOTE, LEFT, "Note: decide whether the previous keeper is alive. Mention the logbook earlier?")
para(SECTION, LEFT, "The Logbook")
para(NORMAL, JUSTIFY, "On the desk lay a logbook bound in green cloth. The last entry was eleven "
     "years old, written in a careful, slanting hand:")
para(QUOTE, LEFT, "Wind north-east, sea rising. The light is lit. Whoever reads this: keep it lit.")
para(SCENE, CENTER, "* * *")
para(NORMAL, JUSTIFY, "That night the storm came in from the north-east, exactly as the logbook "
     "had promised, and Mara kept the light lit.")
para(CHAPTER, CENTER, "Chapter Two")
para(NORMAL, JUSTIFY, "Morning brought a visitor: a boy in a yellow oilskin who had rowed across "
     "from the mainland to ask whether the lighthouse was ", ("haunted", 4), ".")

n = len(text)
out = b"RWP2" + bytes([n & 255, n >> 8 & 255, n >> 16 & 255, 0]) + bytes(text) + bytes(style)
path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "dos", "write", "SAMPLE.WP")
os.makedirs(os.path.dirname(path), exist_ok=True)
open(path, "wb").write(out)
print(path, len(out), "bytes")
