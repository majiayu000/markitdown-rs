Bullets nested five deep, then beyond mammoth's five levels:

* bullet level 0
  + bullet level 1
    - bullet level 2
      * bullet level 3
        + bullet level 4

bullet level 5

bullet level 6

Back to text. Numbered list, restarting after text:

1. one
2. two
   1. two.a
      1. two.a.i
3. three

Mixed kinds at the same level split the list (issue 2323):

1. ordered
   * bullet child
     1. ordered grandchild

* bullet at top

1. ordered at top

Style-based bullets:

* styled bullet
* styled bullet 2

numId without ilvl (issue 1282):

1. no ilvl one
2. no ilvl two

numId 0 means not a list:

not a list item

Unknown numId:

unknown num

Second instance of the decimal list:

1. again one

List items with formatting:

* **bold item** and *italic*
* item with line
  break
* after an empty item

Heading styled list paragraph:

## numbered heading
