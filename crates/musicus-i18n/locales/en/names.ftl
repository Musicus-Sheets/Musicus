# Words of derived part names (a-client-part-names). The instrument names and
# voices are the ensemble's data and are never translated.

# $instruments: the instrument names joined; $key: a key name below.
# Its literal words ("in") are ignored by the library search.
part-name-with-key = { $instruments } in { $key }
# A part without instrument but with key or voices: "Part 2".
part-name-no-instrument = Part
# A part without instrument, key and voices.
part-name-unnamed = Unnamed part
# Between instrument names (spaces kept by the quotes).
part-name-instrument-sep = {" / "}
# Between voices: "Clarinet 1 & 2".
part-name-voice-sep = {" & "}

# Key names in part names, by stored code. Every spelling of every language is
# also a search word for parts in that key.
key-name-Bb = B♭
key-name-Eb = E♭
key-name-F = F
key-name-C = C
key-name-A = A
key-name-D = D
key-name-G = G
key-name-E = E
key-name-Ab = A♭
key-name-Db = D♭
key-name-Gb = G♭
key-name-H = B♮
# Entry of the key selection where it differs from the key name.
key-option-H = B♮ (H)

# Name of the annotation set the reader creates without asking (stored without a name).
annotations-set-default-name = My annotations
