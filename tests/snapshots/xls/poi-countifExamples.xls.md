## POI Tests
| Criteria | Range | Actual | Expected | Comments |
| --- | --- | --- | --- | --- |
| >5 | 4 | 0 | 0 | NaN |
| >5 | 5 | 0 | 0 | NaN |
| >5 | 6 | 1 | 1 | NaN |
| >abc | aba | 0 | 0 | NaN |
| >abc | abc | 0 | 0 | NaN |
| >abc | abd | 1 | 1 | NaN |
| 1 | 1 | 1 | 1 | NaN |
| 1 | 1 | 1 | 1 | NaN |
| 1 | 1 | 1 | 1 | NaN |
| =22 | 22 | 1 | 1 | NaN |
| =22 | 22 | 1 | 1 | NaN |
| 1 | 1 | 1 | 1 | NaN |
| NaN | NaN | 0 | 0 | Blank criteria does not match blank value |
| NaN | NaN | 0 | 0 | Blank criteria does not match empty string |
| NaN | NaN | 1 | 1 | Empty string criteria matches blank value |
| NaN | NaN | 1 | 1 | Empty string criteria matches empty string value |
| = | NaN | 1 | 1 | Explicit equals operator with empty string matches blank value |
| = | NaN | 0 | 0 | Explicit equals operator with empty string does not match empty string value |
| <> | NaN | 1 | 1 | NaN |
| <> | NaN | 0 | 0 | NaN |
| A\*B | A123BC | 0 | 0 | NaN |
| <>A\*B | ABF | 1 | 1 | NaN |
| A~?B | A?B | 1 | 1 | NaN |
| >A~\*B | A~+C | 1 | 1 | NaN |
| 0 | 0 | 1 | 1 | NaN |
| 0 | 0 | 1 | 1 | NaN |
| 0 | NaN | 0 | 0 | NaN |
| 0 | 0 | 1 | 1 | NaN |
| 0 | 0 | 1 | 1 | NaN |
| 0 | NaN | 0 | 0 | NaN |
| 1\*2 | 12342 | 0 | 0 | NaN |
| 1\*2 | 12322 | 1 | 1 | NaN |
| \* | NaN | 1 | 1 | NaN |
| =\* | NaN | 1 | 1 | NaN |
| <> | NaN | 1 | 1 | NaN |
| > | NaN | 0 | 0 | NaN |
| < | NaN | 0 | 0 | NaN |
| <= | NaN | 0 | 0 | NaN |
| >= | NaN | 0 | 0 | NaN |
| TRUE | 1 | 1 | 1 | NaN |
| <TRUE | 0 | 1 | 1 | NaN |
| <TRUE | FALSE | 0 | 0 | NaN |
| <TRUE | TA | 0 | 0 | String 'TA' does not compare with boolean TRUE |
| >FALSE | FB | 0 | 0 | NaN |
| >FALSE | 1 | 1 | 1 | NaN |
| >FALSE | TRUE | 0 | 0 | String 'TRUE' does not compare with boolean FALSE |
| >4t3 | 500 | 0 | 0 | NaN |
| >4t3 | 500 | 1 | 1 | NaN |
| 500 | 500 | 1 | 1 | NaN |
| TRUE | TRUE | 0 | 0 | NaN |
| a\*b | ab | 1 | 1 | NaN |

## MSDN Example 1
| Data | Data.1 | Unnamed: 2 |
| --- | --- | --- |
| apples | 32 | NaN |
| oranges | 54 | NaN |
| peaches | 75 | NaN |
| apples | 86 | NaN |
| Formula | Description | Result |
| 2 | Number of cells with apples in cells A2 through A5. | 2 |
| 1 | Number of cells with peaches in cells A2 through A5. | 1 |
| 3 | Number of cells with oranges and apples in cells A2 through A5. | 3 |
| 2 | Number of cells with a value greater than 55 in cells B2 through B5. | 2 |
| 3 | Number of cells with a value not equal to 75 in cells B2 through B5. | 3 |
| 3 | Number of cells with a value greater than or equal to 32 and less than or equal to 85 in cells B2 through B5. | 3 |

## MSDN Example 2
| Data | Data.1 | Unnamed: 2 |
| --- | --- | --- |
| apples | Yes | NaN |
| NaN | NaN | NaN |
| oranges | NO | NaN |
| peaches | No | NaN |
| NaN | NaN | NaN |
| apples | yes | NaN |
| Formula | Description | Result |
| 4 | Number of cells ending with the letters "es" in cells A2 through A7. | 4 |
| 2 | Number of cells ending with the letters "les" and having exactly 7 letters in cells A2 through A7. | 2 |
| 4 | Number of cells containing any text in cells A2 through A7. | 4 |
| 2 | Number of cells not containing text in cells A2 through A7. | 2 |
| 0.333333 | The average number of No votes (including blank cells) in cells B2 through B7. | 0.333333 |
| 0.5 | The average number of Yes votes (excluding blank cells) in cells B2 through B7. | 0.5 |
