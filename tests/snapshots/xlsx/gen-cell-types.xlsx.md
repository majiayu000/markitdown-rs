## Types
| Name | Value | Kind |
| --- | --- | --- |
| int | 42 | plain |
| float | 3.14159 | plain |
| date14 | 2024-01-01 00:00:00 | builtin 14 |
| date custom | 2024-01-01 12:00:00 | yyyy-mm-dd |
| timedelta | 1 day, 12:00:00 | [h]:mm:ss |
| quoted | 12 | "Week "0 is not a date |
| colour | 1.25 | [Red] stripped |
| datetime22 | 2024-01-01 18:00:00 | m/d/yy h:mm |
| time21 | 12:00:00 | h:mm:ss |
| cjk27 | 45292 | id 27 is not a date for openpyxl |
| escaped | 2024-01-01 00:00:00 | backslash space |
| bool | True | t=b |
| error | NaN | t=e |
| str | formula text | t=str |
| isodate | 2024-02-29 13:45:00 | t=d |
| rich | rich text | runs joined |
| escape | esc\_x000D\_aped | x005F\_ removed |
| padded | padded | strip |
| na string | NaN | NaN |
| tab | tab\there | \t |
| pipe | a | pipe | rPh ignored |
| formula | 2 | cached |
| empty v | NaN | empty |
| percent | 0.25 | 0.00% |
| negative date | 1899-12-29 00:00:00 | before epoch |
| leap1900 | 1900-02-28 00:00:00 | 1900-02-28 |
| serial60 | 1900-02-28 00:00:00 | fake leap day |
| serial61 | 1900-03-01 00:00:00 | 1900-03-01 |
| accounting | -3.5 | not a date |
| mmm | 2024-01-09 00:00:00 | mmm d, yyyy |
