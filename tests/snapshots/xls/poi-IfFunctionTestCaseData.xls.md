## Read Me
| This spreadsheet contains various test cases for lookup functions: VLOOKUP, HLOOKUP, LOOKUP and MATCH |
| --- |
| Name of the main junit test class which uses this spreadsheet: |
| org.apache.poi.ss.formula.functions.TestIFFunctionFromSpreadsheet |
| (The content of cell 'Read Me'!A3 is confirmed by the test) |
| NaN |
| Every sheet besides this first one contains formula evaluation test cases in a standardised format. |
| On row 4 of each sheet, in columns B,C,D there are the column headings "Formula", "Expected Result" and "Comment" |
| The test iterates down from row 5 onward  until a the text "<end>" is found in column A.   Rows with "<skip>" in column A are ignored (useful for currently unsupported behaviour).  Otherwise column A can be used for commenting the group of rows below. |
| If the evaluated result of column B does not match the expected result in column C, the junit test will report a failure.  Test failures get annotated with the section and row comments, if any. |
| Besides the first 4 columns, row 5 onwards, the junit test does not inspect any other cells in the sheet.  The other cells can be used freely to set up test data. |
| Care should be taken to not only make the values in column C match those in column B, but also to make sure column C contains only simple literal values.  This can be achieved easily by 'pasting special' from column B to C, selecting the 'as values' option. |

## IF
| Unnamed: 0 | Sheet Comment: | Simple tests of IF using basic argument values | Unnamed: 3 | Unnamed: 4 | Unnamed: 5 | Unnamed: 6 | Unnamed: 7 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| NaN | NaN | NaN | NaN | NaN | NaN | NaN | NaN |
| NaN | Formula | Expected Result | Comment | Data | NaN | NaN | NaN |
| NaN | NaN | NaN | Basic case | NaN | NaN | NaN | NaN |
| NaN | Fail | Fail | run a test, return one result if TRUE, another if FALSE | 5 | NaN | NaN | NaN |
| NaN | Fail | Fail | NaN | 10 | NaN | NaN | NaN |
| NaN | Pass | Pass | NaN | 15 | NaN | NaN | NaN |
| NaN | Pass | Pass | NaN | 20 | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | NaN | NaN | NaN | NaN |
| NaN | Fail | Fail | reverse of B5:B8, returns same result | 5 | NaN | NaN | NaN |
| NaN | Fail | Fail | NaN | 10 | NaN | NaN | NaN |
| NaN | Pass | Pass | NaN | 15 | NaN | NaN | NaN |
| NaN | Pass | Pass | NaN | 20 | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | NaN | NaN | NaN | NaN |
| NaN | C | C | nested Ifs | 76 | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | NaN | NaN | NaN | NaN |
| NaN | Open | Open | If cell is blank | x | NaN | NaN | NaN |
| NaN | Closed | Closed | NaN | NaN | NaN | NaN | NaN |
| NaN | Closed | Closed | NaN | NaN | NaN | NaN | NaN |
| NaN | Open | Open | NaN | x | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | NaN | NaN | NaN | NaN |
| NaN | 1 | 1 | arrays arguments, 3 args | 1 | NaN | NaN | NaN |
| NaN | 2 | 2 | NaN | 2 | NaN | NaN | NaN |
| NaN | 3 | 3 | NaN | 3 | NaN | NaN | NaN |
| NaN | 0 | 0 | NaN | 4 | NaN | NaN | NaN |
| NaN | 0 | 0 | NaN | 5 | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | NaN | NaN | NaN | NaN |
| NaN | 15 | 15 | bug 62904 | 5 | 12 | NaN | NaN |
| NaN | NaN | NaN | NaN | 10 | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | 15 | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | 20 | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | 25 | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | 30 | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | NaN | NaN | NaN | NaN |
| NaN | 0 | 0 | array arguments, 2 args | 1 | NaN | NaN | NaN |
| NaN | 0 | 0 | NaN | 2 | NaN | NaN | NaN |
| NaN | 0 | 0 | NaN | 3 | NaN | NaN | NaN |
| NaN | 4 | 4 | NaN | 4 | NaN | NaN | NaN |
| NaN | 5 | 5 | NaN | 5 | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | NaN | NaN | NaN | NaN |
| NaN | 1 | 1 | array arguments | False | NaN | NaN | NaN |
| NaN | 2 | 2 | logical test is a reference | False | NaN | NaN | NaN |
| NaN | 3 | 3 | NaN | False | NaN | NaN | NaN |
| NaN | 0 | 0 | NaN | 1 | NaN | NaN | NaN |
| NaN | 0 | 0 | NaN | 1 | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | test data | NaN | NaN | NaN |
| NaN | 2017-07-12 00:00:00 | 2017-07-12 00:00:00 | the last date POI was donated to | 2005-01-01 00:00:00 | POI | 100.0 | 1.0 |
| NaN | 2008-02-25 00:00:00 | 2008-02-25 00:00:00 | the last date Tika was donated to | 2008-02-25 00:00:00 | Tika | 300.0 | 2.0 |
| NaN | NaN | NaN | NaN | 2017-07-12 00:00:00 | POI | 500.0 | 3.0 |
| NaN | 500 | 500 | MAX-IF the largest donation to POI | 2007-08-05 00:00:00 | Tika | 200.0 | 4.0 |
| NaN | 800 | 800 | SUM-IF total donations to POI | 2010-09-10 00:00:00 | POI | 200.0 | 5.0 |
| NaN | 815 | 815 | SUM-IF total donations to POI with 3rd array arg | NaN | NaN | NaN | NaN |
| NaN | 803 | 803 | SUM-IF total donations to POI with 3rd scalar arg | NaN | NaN | NaN | NaN |
| NaN | 200 | 200 | LARGE-IF 2nd largest donation | NaN | NaN | NaN | NaN |
| NaN | 200 | 200 | SMALL-IF 2nd smallest donation | NaN | NaN | NaN | NaN |
| NaN | 12 | 12 | SUM-IF 2nd arg missing | NaN | NaN | NaN | NaN |
| NaN | 3 | 3 | SUM-IF 3rd arg missing | NaN | NaN | NaN | NaN |
| NaN | 100 | 100 | MIN-IF donation to POI | NaN | NaN | NaN | NaN |
| NaN | 300 | 300 | MAX-IF donation to Tika | NaN | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | NaN | NaN | NaN | NaN |
| NaN | 1 | 1 | 2nd arg is error, non-array formula | NaN | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | NaN | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | NaN | NaN | NaN | NaN |
| NaN | NaN | NaN | 2nd arg is error, array formula | 1 | NaN | NaN | NaN |
| NaN | 1 | 1 | NaN | False | NaN | NaN | NaN |
| NaN | NaN | NaN | NaN | NaN | NaN | NaN | NaN |
| <end> | NaN | NaN | NaN | NaN | NaN | NaN | NaN |
