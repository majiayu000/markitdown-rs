Header row repeated (tblHeader):

| Name | Value |
| --- | --- |
| alpha | 1 |
| beta | 2 |

Merged cells:

|  |  |  |
| --- | --- | --- |
| spans two | | c |
| tall | x | y |
| z | w |

Nested table (issue 1248):

|  |  |  |  |  |  |
| --- | --- | --- | --- | --- | --- |
| outer | before   |  |  | | --- | --- | | inner a | inner b | | inner c | inner d |   after |

Pipes, breaks and paragraphs in cells (issue 2438):

|  |  |
| --- | --- |
| a | b | line one line two |
| para one  para two |  |
| **bold** cell | list in cell |

All header rows:

| only | header |
| --- | --- |

Deleted row is dropped:

|  |
| --- |
| kept |

Ragged rows:

|  |  |  |
| --- | --- | --- |
| 1 | 2 | 3 |
| only one |

Bookmark between rows disables merging:

|  |
| --- |
| a |
| b |

Empty table:

End.
