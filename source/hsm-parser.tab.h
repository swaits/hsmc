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
typedef union
{
        char  string[258];
        float constant;
} YYSTYPE;
extern YYSTYPE yylval;
