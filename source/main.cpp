/*
 *  Copyright 2010, Stephen Waits <steve@waits.net>. All rights reserved.
 *  
 *  Redistribution and use in source and binary forms, with or without modification, are
 *  permitted provided that the following conditions are met:
 *  
 *     1. Redistributions of source code must retain the above copyright notice, this list of
 *        conditions and the following disclaimer.
 *  
 *     2. Redistributions in binary form must reproduce the above copyright notice, this list
 *        of conditions and the following disclaimer in the documentation and/or other materials
 *        provided with the distribution.
 *  
 *  THIS SOFTWARE IS PROVIDED BY STEPHEN WAITS ``AS IS'' AND ANY EXPRESS OR IMPLIED
 *  WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
 *  FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL STEPHEN WAITS OR
 *  CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 *  CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 *  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
 *  ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
 *  NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 *  ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *  
 *  The views and conclusions contained in the software and documentation are those of the
 *  authors and should not be interpreted as representing official policies, either expressed
 *  or implied, of Stephen Waits.
 *
 */


#include <stdlib.h>
#include <stdio.h>

#include <vector>
#include <map>
#include <string>
#include <set>

#include "getopt.h"
#include "main.h"
#include "autofile.h"
#include "machine.h"
#include "state.h"
#include "config.h"


//
// globals
//

std::vector<Machine> gMachines;
int                  gCurMachine;
Config               gConfig;
int                  gErrors = 0;



//
// parser interface functions
//

extern "C" FILE* yyin;

extern "C" void parseEndMachine() 
{
	if ( !gMachines[gCurMachine].EndMachine() )
	{
		gErrors++;
	}
}

extern "C" void parseEndState() 
{
	gMachines[gCurMachine].EndState();
}

extern "C" void parseBeginMachine(char* name, int eventsize)
{
	// validate event size
	if ( eventsize <= 0 )
	{
		extern int lineno;
		printf("%d: Invalid event queue size for Machine '%s'\n",
					 lineno,
					 name);
		gErrors++;
	}

	// create new machine
	Machine m;
	m.name = name;
	m.SetQueueSize(eventsize);

	// create top state
	m.AddState("TOPSTATE");

	// add machine to list
	gMachines.push_back(m);

	// update current machine id
	gCurMachine = (int)gMachines.size() - 1;
}

extern "C" void parseBeginState(char* name)
{
	if ( gMachines[gCurMachine].AddState(name) < 0 )
	{
		extern int lineno;
		printf("%d: State '%s' was previously defined\n",
					 lineno,
					 name);
		gErrors++;
	}
}

extern "C" void parseDefault(char* state)
{
	if ( !gMachines[gCurMachine].AddDefault(state) )
	{
		extern int lineno;
		printf("%d: multiple Default states defined in State '%s'\n",
					 lineno,
					 (gMachines[gCurMachine].CurStateName()).c_str());
		gErrors++;
	}
}

extern "C" void parseEntry(char* action)
{
	gMachines[gCurMachine].AddEntry(action);
}

extern "C" void parseIdle(char* action)
{
	gMachines[gCurMachine].AddIdle(action);
}

extern "C" void parseExit(char* action)
{
	gMachines[gCurMachine].AddExit(action);
}

extern "C" void parseTransition(char* event,char* state)
{
	if ( !gMachines[gCurMachine].AddTransition(event,state) )
	{
		extern int lineno;
		printf("%d: multiple Transition's on '%s' defined in '%s'\n",
					 lineno,
					 event,
					 (gMachines[gCurMachine].CurStateName()).c_str());
		gErrors++;
	}
}

extern "C" void parseAction(char* event,char* action)
{
	gMachines[gCurMachine].AddAction(event,action);
}

extern "C" void parseTimeTransition(float time,char* state)
{
	gMachines[gCurMachine].AddTimeTransition(time,state);
}

extern "C" void parseTimeAction(float time,char* action)
{
	gMachines[gCurMachine].AddTimeAction(time,action);
}

extern "C" void parseTerminate(char* event)
{
	if ( !gMachines[gCurMachine].AddTerminate(event) )
	{
		extern int lineno;
		printf("%d: multiple Terminate's on '%s' defined in '%s'\n",
					 lineno,
					 event,
					 (gMachines[gCurMachine].CurStateName()).c_str());
		gErrors++;
	}
}



//
// output functions
//

void OutputEvents(FILE* f, Machine& m)
{
	fprintf(f,
					"\tenum\n"
					"\t{\n"
				 );
	bool first = true;
	for ( std::set<std::string>::iterator it = m.event_refs.begin(); it != m.event_refs.end(); it++ )
	{
		fprintf(f,"\t\t%s%s,\n",(*it).c_str(), first?" = 0":"");
		first = false;
	}
	fprintf(f,
					"\n"
					"\t\t___HSM_NUM_EVENTS___\n"
					"\t};\n\n");
}

void OutputStateIDs(FILE* f, Machine& m)
{
	fprintf(f,
					"\tenum\n"
					"\t{\n"
				 );
	bool first = true;
	for ( std::vector<State>::iterator it = m.states.begin(); it != m.states.end(); it++ )
	{
		fprintf(f,"\t\t%s%s,\n",(*it).name.c_str(), first?" = 0":"");
		first = false;
	}
	fprintf(f,
					"\n"
					"\t\t___HSM_NUM_STATES___\n"
					"\t};\n\n");
}

void OutputPrivateEvents(FILE* f, Machine& m)
{
	// header
	fprintf(f,
					"\tenum\n"
					"\t{\n"
					"\t\t___HSM_ENTRY___ = ___HSM_NUM_EVENTS___,\n"
					"\t\t___HSM_IDLE___,\n"
					"\t\t___HSM_EXIT___,\n"
					"\t\t___HSM_ELICIT_INITIAL_TRANSITION___,\n"
					"\t\t___HSM_ELICIT_PARENT___,\n"
				 );

	// private timer events
	for ( int i=0;i<m.GetTimerDepth();i++ )
	{
		fprintf(f,
						"\t\t___HSM_TIMER_%d___,\n",
						i
					 );
	}

	// footer
	fprintf(f,
					"\n"
					"\t\t___HSM_TOTAL_NUM_EVENTS___\n"
					"\t};\n\n");
}

void OutputActionProtos(FILE* f, Machine& m)
{
	for ( std::set<std::string>::iterator it = m.action_refs.begin(); it != m.action_refs.end(); it++ )
	{
		fprintf(f,"\tvirtual void %s()%s;\n",(*it).c_str(),gConfig.PureVirtuals ? " = 0" : "");
	}
	fprintf(f,"\n");
}

void OutputActions(FILE* f, Machine& m)
{
	fprintf(f,
		"//\n"
		"// actions (must be overridden)\n"
		"//\n"
		);

	for ( std::set<std::string>::iterator it = m.action_refs.begin(); it != m.action_refs.end(); it++ )
	{
		fprintf(f,"void %s::%s()\n",m.name.c_str(),(*it).c_str());
		fprintf(f,
			"{\n"
			"\t// empty\n"
			"}\n\n"
			);
	}
}

void OutputStateProtos(FILE* f, Machine& m, int s)
{
	static int indent = 0;

	fprintf(f,"\tint %*s___HSMSTATE_%s___(int event);\n",indent*4,"",m.states[s].name.c_str());

	indent++;
	for ( unsigned int i=0;i<m.states[s].child.size();i++ )
	{
		OutputStateProtos(f,m,m.states[s].child[i]);
	}
	indent--;
}

void OutputStates(FILE* f, Machine& m)
{
	for ( std::vector<State>::iterator it = m.states.begin(); it != m.states.end(); it++ )
	{
		State& s = (*it);
		std::vector<std::string> null_events;

		// header
		fprintf(f,
						"//\n"
						"// STATE: %s::%s\n"
						"//\n"
						"int %s::___HSMSTATE_%s___(int event)\n"
						"{\n"
						"\tswitch (event)\n"
						"\t{\n",
						m.name.c_str(),s.name.c_str(),
						m.name.c_str(),s.name.c_str());

		fprintf(f,
						"\t\t// internal events\n\n"
					 );

		// entry
		fprintf(f,
						"\t\tcase ___HSM_ENTRY___:\n"
					 );
		if ( gConfig.Debug )
		{
			fprintf(f,
							"\t\t\t___HSM_DEBUG___(\"Entering %s\");\n",
							s.name.c_str()
						 );
		}
		for ( std::map<int,float>::iterator timeit = s.timers.begin(); timeit != s.timers.end(); timeit++ )
		{
			fprintf(f,
							"\t\t\t___HSM_START_TIMER___(%d,%0.7ff);\n",
							(*timeit).first,
							(*timeit).second
						 );
		}
		for ( unsigned int i=0;i<s.entry.size();i++ )
		{
			fprintf(f,
							"\t\t\t%s();\n",
							s.entry[i].c_str()
						 );
		}
		fprintf(f,
						"\t\t\treturn ___HSM_NULL_STATE___;\n"
						"\n"
					 );

		// idle
		if ( s.idle.size() > 0 )
		{
			fprintf(f,
							"\t\tcase ___HSM_IDLE___:\n"
						 );
			for ( unsigned int i=0;i<s.idle.size();i++ )
			{
				fprintf(f,
								"\t\t\t%s();\n",
								s.idle[i].c_str()
							 );
			}
			fprintf(f,
							"\t\t\treturn ___HSM_NULL_STATE___;\n"
							"\n"
						 );
		}

		// exit
		fprintf(f,
						"\t\tcase ___HSM_EXIT___:\n"
					 );
		if ( gConfig.Debug )
		{
			fprintf(f,
							"\t\t\t___HSM_DEBUG___(\"Exiting %s\");\n",
							s.name.c_str()
						 );
		}
		for ( std::map<int,float>::iterator timeit = s.timers.begin(); timeit != s.timers.end(); timeit++ )
		{
			fprintf(f,
							"\t\t\t___HSM_STOP_TIMER___(%d);\n",
							(*timeit).first
						 );
		}
		for ( unsigned int i=0;i<s.exit.size();i++ )
		{
			fprintf(f,
							"\t\t\t%s();\n",
							s.exit[i].c_str()
						 );
		}
		fprintf(f,
						"\t\t\treturn ___HSM_NULL_STATE___;\n"
						"\n"
					 );

		// initial transition (default or history)
		fprintf(f,
						"\t\tcase ___HSM_ELICIT_INITIAL_TRANSITION___:\n"
					 );
		if ( gConfig.Debug )
		{
			fprintf(f,
							"\t\t\t___HSM_DEBUG___(\"In %s\");\n",
							s.name.c_str()
						 );
		}
		fprintf(f,
						"\t\t\treturn %s;\n"
						"\n",
						s.defaultstate != "" ? s.defaultstate.c_str() : "___HSM_NULL_STATE___"
					 );

		// timer events
		if ( s.timers.size() > 0 )
		{
			fprintf(f,
							"\t\t// timer events\n\n"
						 );
			for ( std::map<int,float>::iterator timeit = s.timers.begin(); timeit != s.timers.end(); timeit++ )
			{
				// which timer
				fprintf(f,
								"\t\tcase ___HSM_TIMER_%d___:\n",
								(*timeit).first
							 );

				// all actions with this time
				for ( std::multimap<float,std::string>::iterator acit = s.timeaction.begin(); acit != s.timeaction.end(); acit++ )
				{
					if ( (*acit).first == (*timeit).second )
					{
						// print debug info on action
						if ( gConfig.Debug )
						{
							fprintf(f,
								"\t\t\t___HSM_DEBUG___(\"[TIMER EVENT]: %s()\");\n",
								(*acit).second.c_str()
								);
						}				

						// call timer action
						fprintf(f,
										"\t\t\t%s();\n",
										(*acit).second.c_str()
									 );
					}
				}

				// any transition with this time?
				for ( std::multimap<float,std::string>::iterator trit = s.timetransition.begin(); trit != s.timetransition.end(); trit++ )
				{
					if ( (*trit).first == (*timeit).second )
					{
						// print debug info on transition
						if ( gConfig.Debug )
						{
							fprintf(f,
								"\t\t\t___HSM_DEBUG___(\"[TIMER EVENT]: %s -> %s\");\n",
								s.name.c_str(),
								(*trit).second.c_str()
								);
						}

						// do the transition
						fprintf(f,
										"\t\t\t___HSM_TRANSITION___(%s);\n",
										(*trit).second.c_str()
									 );
						break; // only ONE transition allowed; should never happen, but be safe
					}
				}

				// return event handled
				fprintf(f,
								"\t\t\treturn ___HSM_NULL_STATE___;\n"
								"\n"
							 );
			}
		}

		// user events
		if ( s.terminate.size() > 0 || s.transition.size() > 0 || s.action.size() > 0 )
		{
			fprintf(f,
							"\t\t// user events\n\n"
						 );
		}

		// terminate's if they exist
		for ( std::set<std::string>::iterator it = s.terminate.begin(); it != s.terminate.end(); it++ )
		{
			fprintf(f,
							"\t\tcase %s:\n",
							(*it).c_str()
						 );
		}
		if ( s.terminate.size() > 0 )
		{
			// print debug info on terminal event
			if ( gConfig.Debug )
			{
				fprintf(f,
					"\t\t\t___HSM_DEBUG___(\"[TERMINAL EVENT]: %s -> [HSM TERMINATION]\");\n",
					s.name.c_str()
					);
			}

			// terminate
			fprintf(f,
							"\t\t\t___HSM_STOP_MACHINE___();\n"
							"\t\t\treturn ___HSM_TERMINAL_STATE___;\n"
							"\n"
						 );
		}

		// transition's if they exist
		for ( std::map<std::string,std::string>::iterator it = s.transition.begin(); it != s.transition.end(); it++ )
		{
			fprintf(f,
							"\t\tcase %s:\n",
							(*it).first.c_str()
						 );

			// see if this event is used in any actions
			for ( std::multimap<std::string,std::string>::iterator tit = s.action.lower_bound((*it).first); tit != s.action.upper_bound((*it).first); tit++ )
			{
				// print debug info on action
				if ( gConfig.Debug )
				{
					fprintf(f,
					        "\t\t\t___HSM_DEBUG___(\"%s: %s()\");\n",
					        (*it).first.c_str(),
					        (*tit).second.c_str()
					       );
				}				

				// call action
				fprintf(f,
								"\t\t\t%s();\n",
								(*tit).second.c_str()
							 );

			}

			// print debug info on transition
			if ( gConfig.Debug )
			{
				fprintf(f,
				        "\t\t\t___HSM_DEBUG___(\"%s: %s -> %s\");\n",
				        (*it).first.c_str(),
				        s.name.c_str(),
				        (*it).second.c_str()
				       );
			}

			// now the transition
			fprintf(f,
							"\t\t\t___HSM_TRANSITION___(%s);\n"
							"\t\t\treturn ___HSM_NULL_STATE___;\n\n",
							(*it).second.c_str()
						 );
		}

		// actions if they exist
		for ( std::multimap<std::string,std::string>::iterator it = s.action.begin(); it != s.action.end(); it++ )
		{
			// store off for ease of typing
			std::string event = (*it).first;

			// did this action have a transition too?
			if ( s.transition.find(event) == s.transition.end() )
			{
				// how many actions associated with this event?
				int count = (int)s.action.count(event);

				// print event
				fprintf(f,
								"\t\tcase %s:\n",
								event.c_str()
							 );

				for ( int i=0;i<count;i++ )
				{
					// print debug info on action
					if ( gConfig.Debug )
					{
						fprintf(f,
							"\t\t\t___HSM_DEBUG___(\"%s: %s()\");\n",
							event.c_str(),
							(*it).second.c_str()
							);
					}				

					// call action
					fprintf(f,
									"\t\t\t%s();\n",
									(*it).second.c_str()
								 );
					if ( i < (count - 1) )
					{
						it++;
					}
				}

				fprintf(f,
								"\t\t\treturn ___HSM_NULL_STATE___;\n\n"
							 );
			}
		}

		// default event / return parent
		fprintf(f,
						"\t\t// default event / return parent\n\n"
					 );

		// parent
		fprintf(f,
						"\t\tcase ___HSM_ELICIT_PARENT___:\n"
						"\t\tdefault:\n"
						"\t\t\treturn %s;\n",
						s.parent >= 0 ? m.states[s.parent].name.c_str() : "___HSM_NULL_STATE___"
					 );

		// footer
		fprintf(f,
						"\t}\n"
						"}\n"
						"\n"
					 );
	}
}

void OutputStateTable(FILE* f, Machine& m)
{
	fprintf(f,
					"%s::___HSMSTATE_FUNC___ %s::___HSM_STATE_TAB___[___HSM_NUM_STATES___] = \n"
					"{\n",
					m.name.c_str(),m.name.c_str()
				 );

	for ( unsigned int i=0;i<m.states.size();i++ )
	{
		fprintf(f,
						"\t&%s::___HSMSTATE_%s___,\n",
						m.name.c_str(),m.states[i].name.c_str()
					 );
	}

	fprintf(f,
					"};\n"
					"\n"
				 );
}

void OutputNotice(FILE* f)
{
	fprintf(f,
					"//\n"
					"//  N   N  OOO  TTTTT IIIII  CCCC EEEEE\n"
					"//  NN  N O   O   T     I   C     E\n"
					"//  N N N O   O   T     I   C     EEE\n"
					"//  N  NN O   O   T     I   C     E\n"
					"//  N   N  OOO    T   IIIII  CCCC EEEEE\n"
					"//\n"
					"//  THIS FILE WAS MACHINE GENERATED\n"
					"//\n"
					"//  DO NOT EDIT BY HAND\n"
					"//\n"
					"\n"
				 );
}


void OutputMachineH(FILE* f, Machine& m)
{
	// a warning
	OutputNotice(f);

	// output header
	fprintf(f,
					"#ifndef ___HSMC_GENERATED_%s___\n"
					"#define ___HSMC_GENERATED_%s___\n"
					"\n"
					"class %s\n"
					"{\n"
					"public:\n"
					"\n",
					m.name.c_str(),m.name.c_str(),m.name.c_str()
				 );

	// events
	fprintf(f,
					"\t// events\n"
				 );
	OutputEvents(f,m);

	// states
	fprintf(f,
					"\t// states\n"
				 );
	OutputStateIDs(f,m);

	// public prototypes
	fprintf(f,
					"\t// public declarations\n"
					"\t%s();\n"
					"\tvirtual ~%s();\n"
					"\n"
					"\tint  HSMGetCurrentState() const;\n"
					"\tint  HSMGetParentState(int state) const;\n"
					"\tbool HSMIsInState(int state) const;\n"
					"\tbool HSMIsRunning() const;\n"
					"\n"
					"\tvoid HSMConstruct();\n"
					"\tvoid HSMDestruct();\n"
					"\n"
					"\tbool HSMUpdate(float dt = 0.0f);\n"
					"\n"
					"\tvoid HSMTrigger(int event);\n"
					"\n"
					"\tchar* HSMGetLastDebugMessage();\n"
					"\n",
					m.name.c_str(),
					m.name.c_str()
				 );

	// protected section
	fprintf(f,
					"protected:\n"
					"\n"
				 );

	// actions
	fprintf(f,
					"\t// actions\n"
				 );
	OutputActionProtos(f,m);

	// debug prototype
	fprintf(f,
					"\t// debug hook (override this to trap debug messages)\n"
					"\tvirtual void HSMDebug(char* msg);\n"
					"\n"
				 );

	// private section
	fprintf(f,
					"private:\n"
					"\n"
				 );

	// our event queue class
	fprintf(f,
					"\t// our event queue class\n"
					"\ttemplate<typename T,int N>\n"
					"\tclass StaticQueue\n"
					"\t{\n"
					"\tpublic:\n"
					"\n"
					"\t\tStaticQueue()\n"
					"\t\t{\n"
					"\t\t\tclear();\n"
					"\t\t}\n"
					"\n"
					"\t\tvoid clear()\n"
					"\t\t{\n"
					"\t\t\tfirst = 0;\n"
					"\t\t\tlast  = -1;\n"
					"\n"
					"\t\t\tcursize = 0;\n"
					"\t\t}\n"
					"\n"
					"\t\tbool put(T x)\n"
					"\t\t{\n"
					"\t\t\tif ( size() < N )\n"
					"\t\t\t{\n"
					"\t\t\t\tincrement(last);\n"
					"\t\t\t\tdata[last] = x;\n"
					"\t\t\t\tcursize++;\n"
					"\t\t\t\treturn true;\n"
					"\t\t\t}\n"
					"\t\t\telse\n"
					"\t\t\t{\n"
					"\t\t\t\treturn false;\n"
					"\t\t\t}\n"
					"\t\t}\n"
					"\n"
					"\t\tbool get(T& x)\n"
					"\t\t{\n"
					"\t\t\tif ( size() > 0 )\n"
					"\t\t\t{\n"
					"\t\t\t\tx = data[first];\n"
					"\t\t\t\tincrement(first);\n"
					"\t\t\t\tcursize--;\n"
					"\t\t\t\treturn true;\n"
					"\t\t\t}\n"
					"\t\t\telse\n"
					"\t\t\t{\n"
					"\t\t\t\treturn false;\n"
					"\t\t\t}\n"
					"\t\t}\n"
					"\n"
					"\t\tunsigned int size() const\n"
					"\t\t{\n"
					"\t\t\treturn cursize;\n"
					"\t\t}\n"
					"\n"
					"\t\tbool IsEmpty() const\n"
					"\t\t{\n"
					"\t\t\treturn size() == 0;\n"
					"\t\t}\n"
					"\n"
					"\t\tbool IsFull() const\n"
					"\t\t{\n"
					"\t\t\treturn size() == N;\n"
					"\t\t}\n"
					"\n"
					"\tprivate:\n"
					"\n"
					"\t\tT   data[N];\n"
					"\t\tint first,last,cursize;\n"
					"\n"
					"\t\tvoid increment(int& x)\n"
					"\t\t{\n"
					"\t\t\tif ( ++x >= N )\n"
					"\t\t\t{\n"
					"\t\t\t\tx = 0;\n"
					"\t\t\t}\n"
					"\t\t}\n"
					"\t};\n"
					"\n"
				 );

	// event queue instance
	fprintf(f,
					"\t// event queue\n"
					"\tStaticQueue<int,%d> ___HSM_EVENT_QUEUE___;\n"
					"\n",
					m.GetQueueSize()
				 );

	// debug string pointer
	fprintf(f,
		"\t// debug string pointer\n"
		"\tchar* ___HSM_DEBUG_MSG___;\n"
		"\n"
		);

	// our data
	fprintf(f,
					"\t// current state\n"
					"\tint ___HSM_CURSTATE___;\n"
					"\n"
				 );

	// private events
	fprintf(f,
					"\t// private events\n"
				 );
	OutputPrivateEvents(f,m);

	// a few other #defines
	fprintf(f,
					"\t// special private constants\n"
					"\tenum\n"
					"\t{\n"
					"\t\t___HSM_NULL_STATE___ = -1,\n"
					"\t\t___HSM_TERMINAL_STATE___ = -2,\n"
					"\t\t___HSM_ERROR___ = -3,\n"
					"\t\t___HSM_MAX_DEPTH___ = %d,\n"
					"\t\t___HSM_TIMER_DEPTH___ = %d\n"
					"\t};\n"
					"\n",
					m.GetMaxDepth(),
					m.GetTimerDepth()
				 );

	// timer declaration
	if ( m.GetTimerDepth() > 0 )
	{
		fprintf(f,
						"\t// timers\n"
						"\tfloat ___HSM_TIMERS___[___HSM_TIMER_DEPTH___];\n"
						"\n"
					 );
	}

	// state prototypes
	fprintf(f,
					"\t// state declaration\n"
				 );
	OutputStateProtos(f,m,0);	 // <-- recursive
	fprintf(f,"\n");

	// state table declaration
	fprintf(f,
					"\t// state table declaration\n"
					"\ttypedef int (%s::*___HSMSTATE_FUNC___)(int);\n"
					"\tstatic ___HSMSTATE_FUNC___ ___HSM_STATE_TAB___[___HSM_NUM_STATES___];\n"
					"\n",
					m.name.c_str()
				 );

	// machine driver prototypes
	fprintf(f,
					"\t// machine driver prototypes\n"
					"\tvoid ___HSM_START_MACHINE___();\n"
					"\tvoid ___HSM_STOP_MACHINE___();\n"
					"\tvoid ___HSM_CALC_HIERARCHY___(int state, int path[___HSM_MAX_DEPTH___]);\n"
					"\tvoid ___HSM_TRANSITION___(int state);\n"
					"\tint  ___HSM_CALL_STATE___(int state, int event);\n"
					"\tvoid ___HSM_HANDLE_EVENT___(int event);\n"
					"\tvoid ___HSM_UPDATE_TIMERS___(float dt);\n"
					"\tvoid ___HSM_START_TIMER___(int id, float time);\n"
					"\tvoid ___HSM_STOP_TIMER___(int id);\n"
					"\tvoid ___HSM_DEBUG___(char* msg);\n"
					"\n"
				 );

	// footer
	fprintf(f,
					"};\n"
					"\n"
					"#endif // ___HSMC_GENERATED_%s___\n"
					"\n",
					m.name.c_str()
				 );

}

void OutputMachineHSMDebug(FILE* f, Machine& m)
{
	fprintf(f,
		"//\n"
		"// debug hook (override this to trap debug messages)\n"
		"//\n"
		);

	fprintf(f,
		"void %s::HSMDebug(char* msg)\n"
		"{\n"
		"\t// empty\n"
		"}\n"
		"\n",
		m.name.c_str()
	);
}

void OutputMachineDriverCode(FILE* f, Machine& m)
{
	fprintf(f,
					"%s::%s()\n"
					"{\n"
					"\t___HSM_CURSTATE___ = ___HSM_NULL_STATE___;\n"
					"\t___HSM_DEBUG_MSG___ = \"\";\n"
					"}\n"
					"\n"
					"%s::~%s()\n"
					"{\n"
					"\tif ( ___HSM_CURSTATE___ != ___HSM_NULL_STATE___ )\n"
					"\t{\n"
					"\t\t#if _DEBUG\n"
					"\t\t\tprintf(\"\\n\\nHSM SHOULD BE EXITED via HSMDestruct() BEFORE DESTRUCTION!!!\\n\\n\\n\");\n"
					"\t\t\twhile(1); // infinite loop\n"
					"\t\t#else // _DEBUG\n"
					"\t\t\tHSMDestruct();\n"
					"\t\t#endif // _DEBUG\n"
					"\t}\n"
					"}\n"
					"\n"
					"int %s::HSMGetCurrentState() const\n"
					"{\n"
					"\treturn ___HSM_CURSTATE___;\n"
					"}\n"
					"\n"
					"int %s::HSMGetParentState(int state) const\n"
					"{\n"
					"\t// valid state?\n"
					"\tif ( state >= 0 && state < ___HSM_NUM_STATES___ )\n"
					"\t{\n"
					"\t\t// const_cast used here because we know that pinging a state with ___HSM_ELICIT_PARENT___ is const-safe\n"
					"\t\treturn const_cast<%s*>(this)->___HSM_CALL_STATE___(state,___HSM_ELICIT_PARENT___);\n"
					"\t}\n"
					"\n"
					"\treturn ___HSM_NULL_STATE___;\n"
					"}\n"
					"\n"
					"bool %s::HSMIsInState(int state) const\n"
					"{\n"
					"\tint parent = ___HSM_CURSTATE___;\n"
					"\n"
					"\t// valid state?\n"
					"\tif ( state >= 0 && state < ___HSM_NUM_STATES___ )\n"
					"\t{\n"
					"\t\tdo\n"
					"\t\t{\n"
					"\t\t\tif ( state == parent )\n"
					"\t\t\t{\n"
					"\t\t\t\treturn true;\n"
					"\t\t\t}\n"
					"\t\t}\n"
					"\t\twhile ( (parent = HSMGetParentState(parent)) >= 0 );\n"
					"\t}\n"
					"\n"
					"\treturn false;\n"
					"}\n"
					"\n"
					"bool %s::HSMIsRunning() const\n"
					"{\n"
					"\tint s = HSMGetCurrentState();\n"
					"\treturn (s >= 0 && s < ___HSM_NUM_STATES___);\n"
					"}\n"
					"\n"
					"void %s::HSMConstruct()\n"
					"{\n"
					"\t___HSM_START_MACHINE___();\n"
					"}\n"
					"\n"
					"void %s::HSMDestruct()\n"
					"{\n"
					"\t___HSM_STOP_MACHINE___();\n"
					"}\n"
					"\n"
					"bool %s::HSMUpdate(float dt)\n"
					"{\n"
					"\t\t// TODO - update timers, trigger events as necessary\n"
					"\tif ( HSMIsRunning() )\n"
					"\t{\n"
					"\t\t// update timers\n"
					"\t\t___HSM_UPDATE_TIMERS___(dt);\n"
					"\n"
					"\t\t// process all current events\n"
					"\t\tunsigned int numevents = ___HSM_EVENT_QUEUE___.size();\n"
					"\t\tfor (unsigned int i=0;i<numevents && ___HSM_EVENT_QUEUE___.size() > 0;i++)\n"
					"\t\t{\n"
					"\t\t\tint e = -1;\n"
					"\t\t\tif ( ___HSM_EVENT_QUEUE___.get(e) )\n"
					"\t\t\t{\n"
					"\t\t\t\t___HSM_HANDLE_EVENT___(e);\n"
					"\t\t\t}\n"
					"\t\t}\n"
					"\n"
					"\t\t// now handle our idle event (for the call to Update())\n"
					"\t\t___HSM_HANDLE_EVENT___(___HSM_IDLE___);\n"
					"\t}\n"
					"\n"
					"\t// return if we're still running\n"
					"\treturn HSMIsRunning();\n"
					"}\n"
					"\n"
					"void %s::HSMTrigger(int event)\n"
					"{\n"
					"\tif ( event >= 0 && event < ___HSM_NUM_EVENTS___ )\n"
					"\t{\n"
					"\t\tif ( !___HSM_EVENT_QUEUE___.put(event) )\n"
					"\t\t{\n"
					"\t\t\t___HSM_DEBUG___(\"Event queue overflow!!!\");\n"
					"\t\t}\n"
					"\t}\n"
					"}\n"
					"\n"
					"char* %s::HSMGetLastDebugMessage()\n"
					"{\n"
					"\treturn ___HSM_DEBUG_MSG___;\n"
					"}\n"
					"\n"
					"void %s::___HSM_HANDLE_EVENT___(int event)\n"
					"{\n"
					"\tif ( event >= 0 && event < ___HSM_TOTAL_NUM_EVENTS___ )\n"
					"\t{\n"
					"\t\tint s = ___HSM_CURSTATE___;\n"
					"\t\twhile ( s >= 0 && s < ___HSM_NUM_STATES___ )\n"
					"\t\t{\n"
					"\t\t\ts = ___HSM_CALL_STATE___(s,event);\n"
					"\t\t}\n"
					"\t}\n"
					"}\n"
					"\n"
					"void %s::___HSM_START_MACHINE___()\n"
					"{\n"
					"\tif ( ___HSM_CURSTATE___ == ___HSM_NULL_STATE___ )\n"
					"\t{\n"
					"\t\t// empty event queue\n"
					"\t\t___HSM_EVENT_QUEUE___.clear();\n"
					"\n"
					"\t\t// reset any timers\n"
					"\t\tfor (int i=0;i<___HSM_TIMER_DEPTH___;i++)\n"
					"\t\t{\n"
					"\t\t\t___HSM_STOP_TIMER___(i);\n"
					"\t\t}\n"
					"\n"
					"\t\t// go to first state\n"
					"\t\t___HSM_TRANSITION___(TOPSTATE);\n"
					"\t}\n"
					"}\n"
					"\n"
					"void %s::___HSM_STOP_MACHINE___()\n"
					"{\n"
					"\tif ( ___HSM_CURSTATE___ != ___HSM_NULL_STATE___ )\n"
					"\t{\n"
					"\t\t// exit all states to the root\n"
					"\t\twhile ( ___HSM_CURSTATE___ != TOPSTATE )\n"
					"\t\t{\n"
					"\t\t\t___HSM_CALL_STATE___(___HSM_CURSTATE___,___HSM_EXIT___);\n"
					"\t\t\t___HSM_CURSTATE___ = ___HSM_CALL_STATE___(___HSM_CURSTATE___,___HSM_ELICIT_PARENT___);\n"
					"\t\t}\n"
					"\n"
					"\t\t// and finally exit the root state\n"
					"\t\t___HSM_CALL_STATE___(TOPSTATE,___HSM_EXIT___);\n"
					"\t\t___HSM_CURSTATE___ = ___HSM_NULL_STATE___;\n"
					"\t}\n"
					"}\n"
					"\n"
					"int %s::___HSM_CALL_STATE___(int state, int event)\n"
					"{\n"
					"\tif ( state < 0 || state >= ___HSM_NUM_STATES___ )\n"
					"\t{\n"
					"\t\t// invalid state id\n"
					"\t\treturn ___HSM_ERROR___;\n"
					"\t}\n"
					"\telse\n"
					"\t{\n"
					"\t\t// valid state, lookup state function in table and call it\n"
					"\t\treturn ((*this).*(___HSM_STATE_TAB___[state])) (event);\n"
					"\t}\n"
					"}\n"
					"\n"
					"void %s::___HSM_CALC_HIERARCHY___(int state, int path[___HSM_MAX_DEPTH___])\n"
					"{\n"
					"\tint num = 0;\n"
					"\tint parent;\n"
					"\n"
					"\t// valid state?\n"
					"\tif ( state >= 0 && state < ___HSM_NUM_STATES___ )\n"
					"\t{\n"
					"\t\t// add self\n"
					"\t\tpath[num++] = state;\n"
					"\n"
					"\t\t// add parents\n"
					"\t\tparent = state;\n"
					"\t\twhile ( (parent = ___HSM_CALL_STATE___(parent,___HSM_ELICIT_PARENT___)) >= 0 )\n"
					"\t\t{\n"
					"\t\t\tpath[num++] = parent;\n"
					"\t\t}\n"
					"\t}\n"
					"\n"
					"\t// set remainder to NULL\n"
					"\tfor (int i=num;i<___HSM_MAX_DEPTH___;i++)\n"
					"\t{\n"
					"\t\t// note that an invalid state comes straight here\n"
					"\t\tpath[i] = ___HSM_NULL_STATE___;\n"
					"\t}\n"
					"}\n"
					"\n"
					"void %s::___HSM_TRANSITION___(int state)\n"
					"{\n"
					"\twhile ( state >= 0 && state < ___HSM_NUM_STATES___ )\n"
					"\t{\n"
					"\t\t// determine path to root for each state\n"
					"\t\tint ExitPath[___HSM_MAX_DEPTH___];\n"
					"\t\tint EntryPath[___HSM_MAX_DEPTH___];\n"
					"\t\t___HSM_CALC_HIERARCHY___(___HSM_CURSTATE___,ExitPath);\n"
					"\t\t___HSM_CALC_HIERARCHY___(state,EntryPath);\n"
					"\n"
					"\t\t//\n"
					"\t\t// discover LCA\n"
					"\t\t//\n"
					"\t\tint ientry = ___HSM_MAX_DEPTH___ - 1;\n"
					"\t\tint iexit = ___HSM_MAX_DEPTH___ - 1;\n"
					"\n"
					"\t\t// find root state in each path (or -1)\n"
					"\t\twhile ( ientry >= 0 && EntryPath[ientry] == ___HSM_NULL_STATE___ )\n"
					"\t\t{\n"
					"\t\t\tientry--;\n"
					"\t\t}\n"
					"\t\twhile ( iexit >= 0 && ExitPath[iexit] == ___HSM_NULL_STATE___ )\n"
					"\t\t{\n"
					"\t\t\tiexit--;\n"
					"\t\t}\n"
					"\n"
					"\t\t// beginning at root states, walk backwards until we find the LCA\n"
					"\t\twhile ( ientry >= 0 && iexit >= 0 && EntryPath[ientry] == ExitPath[iexit] )\n"
					"\t\t{\n"
					"\t\t\tientry--;\n"
					"\t\t\tiexit--;\n"
					"\t\t}\n"
					"\n"
					"\t\t// deal with self transition\n"
					"\t\tif ( ___HSM_CURSTATE___ == state && ientry == -1 && iexit == -1 )\n"
					"\t\t{\n"
					"\t\t\tientry++;\n"
					"\t\t\tiexit++;\n"
					"\t\t}\n"
					"\n"
					"\t\t//\n"
					"\t\t// walk transition\n"
					"\t\t//\n"
					"\n"
					"\t\t// call exit routines\n"
					"\t\tfor ( int i=0;i<=iexit;i++ )\n"
					"\t\t{\n"
					"\t\t\t___HSM_CALL_STATE___(ExitPath[i],___HSM_EXIT___);\n"
					"\t\t}\n"
					"\n"
					"\t\t// call entry routines\n"
					"\t\tfor ( int i=ientry;i>=0;i-- )\n"
					"\t\t{\n"
					"\t\t\t___HSM_CALL_STATE___(EntryPath[i],___HSM_ENTRY___);\n"
					"\t\t}\n"
					"\n"
					"\t\t// set current state\n"
					"\t\t___HSM_CURSTATE___ = state;\n"
					"\n"
					"\t\t// now check state for Default transitions\n"
					"\t\tstate = ___HSM_CALL_STATE___(___HSM_CURSTATE___,___HSM_ELICIT_INITIAL_TRANSITION___);\n"
					"\t}\n"
					"}\n"
					"\n"
					"void %s::___HSM_DEBUG___(char* msg)\n"
					"{\n"
					"\t___HSM_DEBUG_MSG___ = msg;\n"
					"\tHSMDebug(msg);\n"
					"}\n"
					"\n",
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str(),
					m.name.c_str()
				 );

	// timer functions
	if ( m.GetTimerDepth() > 0 )
	{
		fprintf(f,
						"void %s::___HSM_UPDATE_TIMERS___(float dt)\n"
						"{\n"
						"\tfor (int i=0;i<___HSM_TIMER_DEPTH___;i++)\n"
						"\t{\n"
						"\t\tif ( ___HSM_TIMERS___[i] >= 0.0f )\n"
						"\t\t{\n"
						"\t\t\t___HSM_TIMERS___[i] -= dt;\n"
						"\t\t\tif ( ___HSM_TIMERS___[i] < 0.0f )\n"
						"\t\t\t{\n"
						"\t\t\t\tif ( !___HSM_EVENT_QUEUE___.put(___HSM_TIMER_0___+i) )\n"
						"\t\t\t\t{\n"
						"\t\t\t\t\t___HSM_DEBUG___(\"Event queue overflow!!!\");\n"
						"\t\t\t\t}\n"
						"\t\t\t}\n"
						"\t\t}\n"
						"\t}\n"
						"}\n"
						"\n"
						"void %s::___HSM_START_TIMER___(int id, float time)\n"
						"{\n"
						"\t___HSM_TIMERS___[id] = time;\n"
						"}\n"
						"\n"
						"void %s::___HSM_STOP_TIMER___(int id)\n"
						"{\n"
						"\t___HSM_TIMERS___[id] = -1.0f;\n"
						"}\n"
						"\n",
						m.name.c_str(),
						m.name.c_str(),
						m.name.c_str()
					 );
	}
	else
	{
		fprintf(f,
						"void %s::___HSM_UPDATE_TIMERS___(float dt)\n"
						"{ // no timers in this machine\n"
						"}\n"
						"\n"
						"void %s::___HSM_START_TIMER___(int id, float time)\n"
						"{ // no timers in this machine\n"
						"}\n"
						"\n"
						"void %s::___HSM_STOP_TIMER___(int id)\n"
						"{ // no timers in this machine\n"
						"}\n"
						"\n",
						m.name.c_str(),
						m.name.c_str(),
						m.name.c_str()
					 );
	}
}

void OutputMachineCPP(FILE* f, std::string incfile, Machine& m){
	// a warning
	OutputNotice(f);

	fprintf(f,
					"#include \"%s\"\n"
					"#if _DEBUG\n"
					"#\tinclude <cstdio>\n"
					"#endif // _DEBUG\n"
					"\n",
					incfile.c_str()
				 );

	// actions
	OutputActions(f,m);

	// HSMDebug() overrideable
	OutputMachineHSMDebug(f,m);

	// state table
	OutputStateTable(f,m);

	// driver code
	OutputMachineDriverCode(f,m);

	// now states
	OutputStates(f,m);
}



//
// main program
//

int main(int argc, char *argv[])
{
	// parse command line
	if ( !gConfig.ParseCommandLine(argc,argv) )
	{
		return 1;
	}

	// header
	if ( !gConfig.Quiet )
	{
		printf("Compiling '%s'...\n",gConfig.InFileName.c_str());
	}

	// open input file
	AutoFILE infile((char*)gConfig.InFileName.c_str(),"r");
	if ( !infile.Opened() )
	{
		printf("ERROR: unable to open '%s'\n",gConfig.InFileName.c_str());
		return 1;
	}

	// parse input file
	yyin = infile;
	if ( yyparse() != 0 )
	{
		printf("ERROR: parser failed\n");
		return 1;
	}

	// bail if we had errors during parse or validation [done at EndMachine()]
	if ( gErrors > 0 )
	{
		printf("\nERROR: compilation aborted due to errors\n");
		return 1;
	}

	// input looks OK!
	if ( !gConfig.Quiet )
	{
		printf("Writing '%s', '%s'...\n",gConfig.OutCPPFileName.c_str(),gConfig.OutHFileName.c_str());
	}

	// open output files
	AutoFILE cppout((char*)gConfig.OutCPPFileName.c_str(),"w");
	if ( !cppout.Opened() )
	{
		printf("ERROR: unable to open '%s'\n",gConfig.OutCPPFileName.c_str());
		return 1;
	}
	AutoFILE hout((char*)gConfig.OutHFileName.c_str(),"w");
	if ( !hout.Opened() )
	{
		printf("ERROR: unable to open '%s'\n",gConfig.OutHFileName.c_str());
		return 1;
	}

	// output code
	for ( std::vector<Machine>::iterator it = gMachines.begin(); it != gMachines.end(); it++ )
	{
		OutputMachineH( hout, *it );
		OutputMachineCPP( cppout, gConfig.IncludeName, *it );
	}

	// done
	if ( !gConfig.Quiet )
	{
		printf("done.\n");
	}
	return 0;
}

