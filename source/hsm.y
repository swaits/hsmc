%{


extern int yylex();
extern void yyerror(char* s);

#include "main.h"

%}

%union
{
        char  string[258];
        float constant;
}

%token MACHINE
%token STATE
%token DEFAULT
%token ENTRY
%token IDLE
%token EXIT
%token TRANSITION
%token ACTION
%token TERMINATE

%token <string> IDENTIFIER
%token <constant> CONSTANT

%%

machines: machine
        | machines machine
        ;
        
machine: machine_decl '{' state_items '}'   
         { 
           parseEndMachine(); 
         }
       ;
       
state_items: state_item
           | state_items state_item
           ;

state_item: state
          | statement
          ;

state: state_decl '{' state_items '}'   
       { 
         parseEndState(); 
       }
     ;
     
statement: default ';'
         | entry ';'
         | idle ';'
         | exit ';'
         | transition ';'
         | action ';'
         | timetransition ';'
         | timeaction ';'
         | terminate ';'
         ;
         
machine_decl: MACHINE '(' IDENTIFIER ',' CONSTANT ')'   
              { 
                parseBeginMachine($3,(int)$5);
              }
            ;
            
state_decl: STATE '(' IDENTIFIER ')'  
            { 
              parseBeginState($3);
            }
          ;
         
default: DEFAULT '(' IDENTIFIER ')'
         {
           parseDefault($3);
         }
       ;
        
entry: ENTRY '(' IDENTIFIER ')'  
       { 
         parseEntry($3);
       }
     ;
        
idle: IDLE '(' IDENTIFIER ')'
      {
        parseIdle($3);
      }
    ;
        
exit: EXIT '(' IDENTIFIER ')'
      {
        parseExit($3);
      }
    ;
        
transition: TRANSITION '(' IDENTIFIER ',' IDENTIFIER ')'
            { 
              parseTransition($3,$5);
            }
          ;
        
action: ACTION '(' IDENTIFIER ',' IDENTIFIER ')'
        {
          parseAction($3,$5);
        }
      ;
        
timetransition: TRANSITION '(' CONSTANT ',' IDENTIFIER ')'
                { 
                  parseTimeTransition($3,$5);
                }
              ;
        
timeaction: ACTION '(' CONSTANT ',' IDENTIFIER ')'
            {
              parseTimeAction($3,$5);
            }
          ;
        
terminate: TERMINATE '(' IDENTIFIER ')'
           {
             parseTerminate($3);
           }
         ;

       
       

