#ifndef lint
static char yysccsid[] = "@(#)yaccpar	1.9 (Berkeley) 02/21/93";
#endif
#define YYBYACC 1
#define YYMAJOR 1
#define YYMINOR 9
#define yyclearin (yychar=(-1))
#define yyerrok (yyerrflag=0)
#define YYRECOVERING (yyerrflag!=0)
#define YYPREFIX "yy"
#line 2 "hsm.y"


extern int yylex();
extern void yyerror(char* s);

#include "main.h"

#line 11 "hsm.y"
typedef union
{
        char  string[258];
        float constant;
} YYSTYPE;
#line 26 "hsm-parser.tab.c"
#define MACHINE 257
#define STATE 258
#define DEFAULT 259
#define ENTRY 260
#define IDLE 261
#define EXIT 262
#define TRANSITION 263
#define ACTION 264
#define TERMINATE 265
#define IDENTIFIER 266
#define CONSTANT 267
#define YYERRCODE 256
short yylhs[] = {                                        -1,
    0,    0,    1,    3,    3,    4,    4,    5,    6,    6,
    6,    6,    6,    6,    6,    6,    6,    2,    7,    8,
    9,   10,   11,   12,   13,   14,   15,   16,
};
short yylen[] = {                                         2,
    1,    2,    4,    1,    2,    1,    1,    4,    2,    2,
    2,    2,    2,    2,    2,    2,    2,    6,    4,    4,
    4,    4,    4,    6,    6,    6,    6,    4,
};
short yydefred[] = {                                      0,
    0,    0,    1,    0,    0,    2,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    4,    6,    7,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    3,
    5,    0,    9,   10,   11,   12,   13,   14,   15,   16,
   17,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,   18,   19,   20,   21,   22,   23,    0,
    0,    0,    0,   28,    8,    0,    0,    0,    0,   24,
   26,   25,   27,
};
short yydgoto[] = {                                       2,
    3,    4,   17,   18,   19,   20,   21,   22,   23,   24,
   25,   26,   27,   28,   29,   30,
};
short yysindex[] = {                                   -251,
  -33, -251,    0, -106, -248,    0, -249,  -25,  -20,  -19,
  -18,  -17,  -15,  -14,  -13,  -12, -125,    0,    0,    0,
  -99,  -30,  -29,  -28,  -27,  -26,  -24,  -23,  -22,  -21,
 -233, -227, -226, -225, -224, -223, -264, -262, -222,    0,
    0, -249,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    4,    5,    7,    8,    9,   10,   11,   12,   13,
   14,   18, -117,    0,    0,    0,    0,    0,    0, -214,
 -213, -212, -206,    0,    0,   20,   21,   22,   23,    0,
    0,    0,    0,
};
short yyrindex[] = {                                      0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,
};
short yygindex[] = {                                      0,
   63,    0,   24,  -16,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,
};
#define YYTABLESIZE 148
short yytable[] = {                                      40,
   41,   58,   59,   60,   61,    1,    5,   75,    9,   10,
   11,   12,   13,   14,   15,   16,    7,    8,   31,   32,
   33,   34,   35,   42,   36,   37,   38,   39,   43,   44,
   45,   46,   47,   52,   48,   49,   50,   51,   53,   54,
   55,   56,   57,   62,   64,   65,   41,   66,   67,   68,
   69,   76,   77,   78,   70,   71,   72,   73,   74,   79,
   80,   81,   82,   83,    6,   63,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    0,    0,    0,    0,    0,    0,    0,    0,
    0,    0,    9,   10,   11,   12,   13,   14,   15,   16,
    9,   10,   11,   12,   13,   14,   15,   16,
};
short yycheck[] = {                                     125,
   17,  266,  267,  266,  267,  257,   40,  125,  258,  259,
  260,  261,  262,  263,  264,  265,  123,  266,   44,   40,
   40,   40,   40,  123,   40,   40,   40,   40,   59,   59,
   59,   59,   59,  267,   59,   59,   59,   59,  266,  266,
  266,  266,  266,  266,   41,   41,   63,   41,   41,   41,
   41,  266,  266,  266,   44,   44,   44,   44,   41,  266,
   41,   41,   41,   41,    2,   42,   -1,   -1,   -1,   -1,
   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,
   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,
   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,
   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,
   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,
   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,   -1,
   -1,   -1,  258,  259,  260,  261,  262,  263,  264,  265,
  258,  259,  260,  261,  262,  263,  264,  265,
};
#define YYFINAL 2
#ifndef YYDEBUG
#define YYDEBUG 0
#endif
#define YYMAXTOKEN 267
#if YYDEBUG
char *yyname[] = {
"end-of-file",0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,"'('","')'",0,0,"','",0,0,0,0,0,0,0,0,0,0,0,0,0,0,"';'",0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,"'{'",0,"'}'",0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,"MACHINE",
"STATE","DEFAULT","ENTRY","IDLE","EXIT","TRANSITION","ACTION","TERMINATE",
"IDENTIFIER","CONSTANT",
};
char *yyrule[] = {
"$accept : machines",
"machines : machine",
"machines : machines machine",
"machine : machine_decl '{' state_items '}'",
"state_items : state_item",
"state_items : state_items state_item",
"state_item : state",
"state_item : statement",
"state : state_decl '{' state_items '}'",
"statement : default ';'",
"statement : entry ';'",
"statement : idle ';'",
"statement : exit ';'",
"statement : transition ';'",
"statement : action ';'",
"statement : timetransition ';'",
"statement : timeaction ';'",
"statement : terminate ';'",
"machine_decl : MACHINE '(' IDENTIFIER ',' CONSTANT ')'",
"state_decl : STATE '(' IDENTIFIER ')'",
"default : DEFAULT '(' IDENTIFIER ')'",
"entry : ENTRY '(' IDENTIFIER ')'",
"idle : IDLE '(' IDENTIFIER ')'",
"exit : EXIT '(' IDENTIFIER ')'",
"transition : TRANSITION '(' IDENTIFIER ',' IDENTIFIER ')'",
"action : ACTION '(' IDENTIFIER ',' IDENTIFIER ')'",
"timetransition : TRANSITION '(' CONSTANT ',' IDENTIFIER ')'",
"timeaction : ACTION '(' CONSTANT ',' IDENTIFIER ')'",
"terminate : TERMINATE '(' IDENTIFIER ')'",
};
#endif
#ifdef YYSTACKSIZE
#undef YYMAXDEPTH
#define YYMAXDEPTH YYSTACKSIZE
#else
#ifdef YYMAXDEPTH
#define YYSTACKSIZE YYMAXDEPTH
#else
#define YYSTACKSIZE 500
#define YYMAXDEPTH 500
#endif
#endif
int yydebug;
int yynerrs;
int yyerrflag;
int yychar;
short *yyssp;
YYSTYPE *yyvsp;
YYSTYPE yyval;
YYSTYPE yylval;
short yyss[YYSTACKSIZE];
YYSTYPE yyvs[YYSTACKSIZE];
#define yystacksize YYSTACKSIZE
#define YYABORT goto yyabort
#define YYREJECT goto yyabort
#define YYACCEPT goto yyaccept
#define YYERROR goto yyerrlab
int
yyparse()
{
    register int yym, yyn, yystate;
#if YYDEBUG
    register char *yys;
    extern char *getenv();

    if (yys = getenv("YYDEBUG"))
    {
        yyn = *yys;
        if (yyn >= '0' && yyn <= '9')
            yydebug = yyn - '0';
    }
#endif

    yynerrs = 0;
    yyerrflag = 0;
    yychar = (-1);

    yyssp = yyss;
    yyvsp = yyvs;
    *yyssp = yystate = 0;

yyloop:
    if (yyn = yydefred[yystate]) goto yyreduce;
    if (yychar < 0)
    {
        if ((yychar = yylex()) < 0) yychar = 0;
#if YYDEBUG
        if (yydebug)
        {
            yys = 0;
            if (yychar <= YYMAXTOKEN) yys = yyname[yychar];
            if (!yys) yys = "illegal-symbol";
            printf("%sdebug: state %d, reading %d (%s)\n",
                    YYPREFIX, yystate, yychar, yys);
        }
#endif
    }
    if ((yyn = yysindex[yystate]) && (yyn += yychar) >= 0 &&
            yyn <= YYTABLESIZE && yycheck[yyn] == yychar)
    {
#if YYDEBUG
        if (yydebug)
            printf("%sdebug: state %d, shifting to state %d\n",
                    YYPREFIX, yystate, yytable[yyn]);
#endif
        if (yyssp >= yyss + yystacksize - 1)
        {
            goto yyoverflow;
        }
        *++yyssp = yystate = yytable[yyn];
        *++yyvsp = yylval;
        yychar = (-1);
        if (yyerrflag > 0)  --yyerrflag;
        goto yyloop;
    }
    if ((yyn = yyrindex[yystate]) && (yyn += yychar) >= 0 &&
            yyn <= YYTABLESIZE && yycheck[yyn] == yychar)
    {
        yyn = yytable[yyn];
        goto yyreduce;
    }
    if (yyerrflag) goto yyinrecovery;
#ifdef lint
    goto yynewerror;
#endif
yynewerror:
    yyerror("syntax error");
#ifdef lint
    goto yyerrlab;
#endif
yyerrlab:
    ++yynerrs;
yyinrecovery:
    if (yyerrflag < 3)
    {
        yyerrflag = 3;
        for (;;)
        {
            if ((yyn = yysindex[*yyssp]) && (yyn += YYERRCODE) >= 0 &&
                    yyn <= YYTABLESIZE && yycheck[yyn] == YYERRCODE)
            {
#if YYDEBUG
                if (yydebug)
                    printf("%sdebug: state %d, error recovery shifting\
 to state %d\n", YYPREFIX, *yyssp, yytable[yyn]);
#endif
                if (yyssp >= yyss + yystacksize - 1)
                {
                    goto yyoverflow;
                }
                *++yyssp = yystate = yytable[yyn];
                *++yyvsp = yylval;
                goto yyloop;
            }
            else
            {
#if YYDEBUG
                if (yydebug)
                    printf("%sdebug: error recovery discarding state %d\n",
                            YYPREFIX, *yyssp);
#endif
                if (yyssp <= yyss) goto yyabort;
                --yyssp;
                --yyvsp;
            }
        }
    }
    else
    {
        if (yychar == 0) goto yyabort;
#if YYDEBUG
        if (yydebug)
        {
            yys = 0;
            if (yychar <= YYMAXTOKEN) yys = yyname[yychar];
            if (!yys) yys = "illegal-symbol";
            printf("%sdebug: state %d, error recovery discards token %d (%s)\n",
                    YYPREFIX, yystate, yychar, yys);
        }
#endif
        yychar = (-1);
        goto yyloop;
    }
yyreduce:
#if YYDEBUG
    if (yydebug)
        printf("%sdebug: state %d, reducing by rule %d (%s)\n",
                YYPREFIX, yystate, yyn, yyrule[yyn]);
#endif
    yym = yylen[yyn];
    yyval = yyvsp[1-yym];
    switch (yyn)
    {
case 3:
#line 37 "hsm.y"
{ 
           parseEndMachine(); 
         }
break;
case 8:
#line 51 "hsm.y"
{ 
         parseEndState(); 
       }
break;
case 18:
#line 68 "hsm.y"
{ 
                parseBeginMachine(yyvsp[-3].string,(int)yyvsp[-1].constant);
              }
break;
case 19:
#line 74 "hsm.y"
{ 
              parseBeginState(yyvsp[-1].string);
            }
break;
case 20:
#line 80 "hsm.y"
{
           parseDefault(yyvsp[-1].string);
         }
break;
case 21:
#line 86 "hsm.y"
{ 
         parseEntry(yyvsp[-1].string);
       }
break;
case 22:
#line 92 "hsm.y"
{
        parseIdle(yyvsp[-1].string);
      }
break;
case 23:
#line 98 "hsm.y"
{
        parseExit(yyvsp[-1].string);
      }
break;
case 24:
#line 104 "hsm.y"
{ 
              parseTransition(yyvsp[-3].string,yyvsp[-1].string);
            }
break;
case 25:
#line 110 "hsm.y"
{
          parseAction(yyvsp[-3].string,yyvsp[-1].string);
        }
break;
case 26:
#line 116 "hsm.y"
{ 
                  parseTimeTransition(yyvsp[-3].constant,yyvsp[-1].string);
                }
break;
case 27:
#line 122 "hsm.y"
{
              parseTimeAction(yyvsp[-3].constant,yyvsp[-1].string);
            }
break;
case 28:
#line 128 "hsm.y"
{
             parseTerminate(yyvsp[-1].string);
           }
break;
#line 414 "hsm-parser.tab.c"
    }
    yyssp -= yym;
    yystate = *yyssp;
    yyvsp -= yym;
    yym = yylhs[yyn];
    if (yystate == 0 && yym == 0)
    {
#if YYDEBUG
        if (yydebug)
            printf("%sdebug: after reduction, shifting from state 0 to\
 state %d\n", YYPREFIX, YYFINAL);
#endif
        yystate = YYFINAL;
        *++yyssp = YYFINAL;
        *++yyvsp = yyval;
        if (yychar < 0)
        {
            if ((yychar = yylex()) < 0) yychar = 0;
#if YYDEBUG
            if (yydebug)
            {
                yys = 0;
                if (yychar <= YYMAXTOKEN) yys = yyname[yychar];
                if (!yys) yys = "illegal-symbol";
                printf("%sdebug: state %d, reading %d (%s)\n",
                        YYPREFIX, YYFINAL, yychar, yys);
            }
#endif
        }
        if (yychar == 0) goto yyaccept;
        goto yyloop;
    }
    if ((yyn = yygindex[yym]) && (yyn += yystate) >= 0 &&
            yyn <= YYTABLESIZE && yycheck[yyn] == yystate)
        yystate = yytable[yyn];
    else
        yystate = yydgoto[yym];
#if YYDEBUG
    if (yydebug)
        printf("%sdebug: after reduction, shifting from state %d \
to state %d\n", YYPREFIX, *yyssp, yystate);
#endif
    if (yyssp >= yyss + yystacksize - 1)
    {
        goto yyoverflow;
    }
    *++yyssp = yystate;
    *++yyvsp = yyval;
    goto yyloop;
yyoverflow:
    yyerror("yacc stack overflow");
yyabort:
    return (1);
yyaccept:
    return (0);
}
