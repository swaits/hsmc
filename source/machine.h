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


#ifndef __machine_h__
#define __machine_h__


#include <string>
#include <vector>
#include <map>
#include <set>

#include "state.h"

class Machine
{
public:

	// data
	std::string name;

	std::vector<State> states;
	std::map<std::string,int> statemap;

	std::set<std::string> event_refs;
	std::set<std::string> state_refs;
	std::set<std::string> action_refs;

	// construction
	Machine();

	// add data
	int AddState(std::string name);
	int EndState();
	bool AddDefault(std::string state);
	bool AddEntry(std::string action);
	bool AddIdle(std::string action);
	bool AddExit(std::string action);
	bool AddTransition(char* event,char* state);
	bool AddAction(char* event,char* action);
	bool AddTimeTransition(float time,char* state);
	bool AddTimeAction(float time,char* action);
	bool AddTerminate(char* event);

	// get data
	std::string CurStateName();
	int GetMaxDepth();
	int GetTimerDepth();

	// finalize machine
	bool EndMachine();

	// settings
	void SetQueueSize(int size);
	int  GetQueueSize();

private:

	int curstate;
	int maxdepth;
	int depth;
	int timerdepth;
	int queuesize;

	int CountTimers(int sid);

	bool Validate();
};


// initialize a machine
Machine::Machine()
{
	// set current state to invalid
	curstate = -1;

	// default depth
	maxdepth = 0;
	depth = 0;
}

int Machine::AddState(std::string name)
{
	// make sure this isn't a duplicate state
	if ( statemap.find(name) != statemap.end() )
	{
		return -1;
	}

	// figure out our new state id
	int id = (int)states.size();

	// add this state to our name -> id map
	statemap[name] = id;

	// tell parent about this new child state
	if ( curstate >= 0 )
	{
		states[curstate].child.push_back(id); 
	}

	// create new state
	State s;
	s.name = name;

	// tell current state about parent
	s.parent = curstate;

	// add state to list and make current
	states.push_back(s);

	// record depth
	depth++;
	if ( depth > maxdepth )
	{
		maxdepth = depth;
	}

	// make this new state the current state
	curstate = id;
	return curstate;
}

int Machine::EndState()
{
	// dec depth
	depth--;

	// move curstate to parent
	if ( curstate >= 0 )
	{
		curstate = states[curstate].parent;
	}
	return curstate;
}

bool Machine::AddDefault(std::string state)
{
	if ( curstate < 0 )
	{
		return false;
	}
	else if ( states[curstate].defaultstate != "" )
	{
		// default state already set for this state
		return false;
	}
	else
	{
		states[curstate].defaultstate = state;
		state_refs.insert(state);
		return true;
	}
}

bool Machine::AddEntry(std::string action)
{
	if ( curstate < 0 )
	{
		return false;
	}

	// add entry action and reference
	states[curstate].entry.push_back(action);
	action_refs.insert(action);

	return true;
}

bool Machine::AddIdle(std::string action)
{
	if ( curstate < 0 )
	{
		return false;
	}

	// add idle action and reference
	states[curstate].idle.push_back(action);
	action_refs.insert(action);

	return true;
}

bool Machine::AddExit(std::string action)
{
	if ( curstate < 0 )
	{
		return false;
	}

	// add exit action and reference
	states[curstate].exit.push_back(action);
	action_refs.insert(action);

	return true;
}

bool Machine::AddTransition(char* event,char* state)
{
	if ( curstate < 0 )
	{
		return false;
	}

	// make certain a transition doesn't already exist for this event
	if ( states[curstate].transition.find(event) != states[curstate].transition.end() )
	{
		// transition already exists for this event
		return false;
	}

	// add transition
	states[curstate].transition[event] = state;

	// add references
	event_refs.insert(event);
	state_refs.insert(state);

	return true;
}

bool Machine::AddAction(char* event,char* action)
{
	if ( curstate < 0 )
	{
		return false;
	}

	// add action
	states[curstate].action.insert( std::pair<std::string,std::string>(event,action) );

	// add references
	event_refs.insert(event);
	action_refs.insert(action);

	return true;
}

bool Machine::AddTimeTransition(float time,char* state)
{
	if ( curstate < 0 )
	{
		return false;
	}

	// add timeout
	states[curstate].timetransition.insert( std::pair<float,std::string>(time,state) );

	// add references
	state_refs.insert(state);

	return true;
}

bool Machine::AddTimeAction(float time,char* action)
{
	if ( curstate < 0 )
	{
		return false;
	}

	// add timeout
	states[curstate].timeaction.insert( std::pair<float,std::string>(time,action) );

	// add references
	action_refs.insert(action);

	return true;
}

bool Machine::AddTerminate(char* event)
{
	if ( curstate < 0 )
	{
		return false;
	}
	else if ( states[curstate].terminate.find(event) != states[curstate].terminate.end() )
	{
		// already exists
		return false;
	}

	// add terminate
	states[curstate].terminate.insert(event);

	// add reference
	event_refs.insert(event);

	return true;
}

std::string Machine::CurStateName()
{
	if ( curstate < 0 )
	{
		return "";
	}
	else
	{
		return states[curstate].name;
	}
}

int Machine::GetMaxDepth()
{
	return maxdepth;
}

int Machine::GetTimerDepth()
{
	return timerdepth;
}

bool Machine::EndMachine()
{
	// now let's validate it
	return Validate();
}

int Machine::CountTimers(int sid)
{
	int count = 0;
	while ( sid >= 0 )
	{
		count += (int)states[sid].timers.size();
		sid = states[sid].parent;
	}

	return count;
}


bool Machine::Validate()
{
	bool ok = true;

	// make sure all referenced states are defined
	for ( std::set<std::string>::iterator it = state_refs.begin(); it != state_refs.end(); it++ )
	{
		if ( statemap.find( *it ) == statemap.end() )
		{
			ok = false;
			printf(
						"Error: State '%s' referenced but not defined in Machine '%s'\n", 
						(*it).c_str(),name.c_str());
		}
	}

	// make sure events listed as terminate are not also listed in actions or transitions
	for ( std::vector<State>::iterator it = states.begin(); it != states.end(); it++ )
	{
		// for convenience
		State& s = (*it);

		// check each "terminate event and make sure it's not in the action or transition list
		for ( std::set<std::string>::iterator term = s.terminate.begin(); term != s.terminate.end(); term++ )
		{
			// look in action & transitions lists
			if ( s.action.find(*term) != s.action.end() || s.transition.find(*term) != s.transition.end() )
			{
				ok = false;
				printf(
							"Error: Terminate event '%s' is not unique in State '%s', Machine '%s'\n",
							(*term).c_str(), s.name.c_str(), name.c_str()
							);
			}
		}
	}

	// make sure all of the state names are NOT event names
	for ( std::vector<State>::iterator it = states.begin(); it != states.end(); it++ )
	{
		State& s = (*it);

		// look for this name in the event list
		if ( event_refs.find(s.name) != event_refs.end() )
		{
			ok = false;
			printf(
						"Error: State-Event name collision on '%s' in Machine '%s'\n",
						s.name.c_str(),
						name.c_str()
						);
		}
	}

	// make certain that no timed actions or transitions exist after shortest timed
	// transition in each state
	for ( std::vector<State>::iterator it = states.begin(); it != states.end(); it++ )
	{
		State& s = (*it);

		// if no timed transitions we're done
		if ( s.timetransition.size() == 0 )
		{
			continue;
		}

		// find shortest timed transition
		std::multimap<float,std::string>::iterator ttit = s.timetransition.begin();
		float shorttime = (*ttit).first;
		std::string shorttrans = (*ttit).second;

		// now iterate timed transitions and warn about remaining
		for ( ttit++; ttit != s.timetransition.end(); ttit++ )
		{
			printf(
						"Error: Timed Transition '%s/%f' superceded by '%s/%f' in State '%s', Machine '%s'\n",
						(*ttit).second.c_str(),
						(*ttit).first,
						shorttrans.c_str(),
						shorttime,
						s.name.c_str(),
						name.c_str()
						);
			s.timetransition.erase(ttit);
			ok = false;
		}

		// iterate through timed actions and warn about anything over
		std::multimap<float,std::string>::iterator tait;
		for ( tait = s.timeaction.begin(); tait != s.timeaction.end(); tait++ )
		{
			if ( (*tait).first >= shorttime )
			{
				printf(
							"Error: Timed Action '%s/%f' superceded by '%s/%f' in State '%s', Machine '%s'\n",
							(*tait).second.c_str(),
							(*tait).first,
							shorttrans.c_str(),
							shorttime,
							s.name.c_str(),
							name.c_str()
							);
				s.timeaction.erase(tait);
				ok = false;
			}
		}
	}

	// now assign timers (value+id) for each state
	for ( std::vector<State>::iterator it = states.begin(); it != states.end(); it++ )
	{
		State& s = (*it);

		// now count up timer values for this state
		std::set<float> timervalues;
		for ( std::multimap<float,std::string>::iterator ttit = s.timetransition.begin(); ttit != s.timetransition.end(); ttit++ )
		{
			timervalues.insert( (*ttit).first );
		}
		for ( std::multimap<float,std::string>::iterator tait = s.timeaction.begin(); tait != s.timeaction.end(); tait++ )
		{
			timervalues.insert( (*tait).first );
		}

		// assign timer id's to values
		int ptimers = CountTimers(s.parent);
		for ( std::set<float>::iterator myit = timervalues.begin(); myit != timervalues.end(); myit++ )
		{
			s.timers[ptimers++] = *myit;
		}
	}

	// determine max timer depth
	timerdepth = 0;
	for ( unsigned int i=0;i<states.size();i++ )
	{
		int depth = CountTimers(i);
		if ( depth > timerdepth )
		{
			timerdepth = depth;
		}
	}

	return ok;
}

void Machine::SetQueueSize(int size)
{
	queuesize = size;
}

int  Machine::GetQueueSize()
{
	return queuesize;
}

#endif // __machine_h__


