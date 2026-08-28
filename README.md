# voicemeeter-config

Reads and writes Voicemeeter's XML settings files.

Covers strips, buses, the routing matrix, compressor and gate values, device
assignments, labels, scene layers and the menu options.

## Round tripping

A file read and written again keeps what it came in with. Elements this crate
does not model are carried through untouched rather than dropped, so loading
someone's settings and saving them does not quietly delete the parts belonging
to features it has no opinion about.

The writer escapes text and attributes. A label containing `&` or `<` would
otherwise produce a file the parser then refuses to read.

## Two parsers, on purpose

Reading uses a DOM, because the importer looks elements up by which attributes
they carry rather than reading them in order. The DOM crate cannot write, hence
the second XML dependency for output.

## License

Public domain. See UNLICENSE.
