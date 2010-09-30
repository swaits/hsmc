#ifndef __main_h__
#define __main_h__

#if defined(__cplusplus)
extern "C" {
#endif // defined(__cplusplus)

	void parseEndMachine();
	void parseEndState();
	void parseBeginMachine(char* name,int eventsize);
	void parseBeginState(char* name);
	void parseDefault(char* state);
	void parseEntry(char* action);
	void parseIdle(char* action);
	void parseExit(char* action);
	void parseTransition(char* event,char* state);
	void parseAction(char* event,char* action);
	void parseTimeTransition(float time,char* state);
	void parseTimeAction(float time,char* action);
	void parseTerminate(char* event);

	int yyparse();

#if defined(__cplusplus)
}
#endif // defined(__cplusplus)

#endif // __main_h__

