%{


#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "hsm-parser.tab.h"

int lineno = 1;

void yyerror(char* s);


%}


%%

  /* whitespace */

[ \t]+                                    ; /* skip whitespace */

  

  /* comments */

"//".*                                    ; /* skip C++ comments */


  /* punctuation */

","                                       { return ','; }
";"                                       { return ';'; }
"{"                                       { return '{'; }
"}"                                       { return '}'; }
"("                                       { return '('; }
")"                                       { return ')'; }


  /* keywords */

[Mm][Aa][Cc][Hh][Ii][Nn][Ee]              { return MACHINE; }
[Ss][Tt][Aa][Tt][Ee]                      { return STATE; }
[Dd][Ee][Ff][Aa][Uu][Ll][Tt]              { return DEFAULT; }
[Ee][Nn][Tt][Rr][Yy]                      { return ENTRY; }
[Ii][Dd][Ll][Ee]                          { return IDLE; }
[Ee][Xx][Ii][Tt]                          { return EXIT; }
[Tt][Rr][Aa][Nn][Ss][Ii][Tt][Ii][Oo][Nn]  { return TRANSITION; }
[Aa][Cc][Tt][Ii][Oo][Nn]                  { return ACTION; }
[Tt][Ee][Rr][Mm][Ii][Nn][Aa][Tt][Ee]      { return TERMINATE; }


  /* identifiers */

[a-zA-Z_][a-zA-Z_0-9]*                    { if ( strlen(yytext) > 256 ) printf("%d: truncating symbol '%s' to 256 characters\n",lineno,yytext); strncpy(yylval.string,yytext,256); return IDENTIFIER; }

  
  /* constants */
  
[0-9]+\.?[0-9]*                           { yylval.constant = (float)atof(yytext); return CONSTANT; }


  /* newlines, etc. */

\n                                        { lineno++; }
.                                         ; /* ignore */


%%



int yywrap()
{
        return 1;
}

void yyerror(char* s)
{
        printf("\n%d: %s at %s\n", lineno, s, yytext);
}




