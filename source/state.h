#ifndef __state_h__
#define __state_h__

#include <string>
#include <vector>
#include <map>

class State
{
public:

	std::string name;

	int parent;
	std::vector<int> child;

	std::string defaultstate;
	std::vector<std::string> entry;
	std::vector<std::string> idle;
	std::vector<std::string> exit;
	std::map<std::string,std::string> transition;
	std::multimap<std::string,std::string> action;
	std::set<std::string> terminate;
	std::multimap<float,std::string> timetransition;
	std::multimap<float,std::string> timeaction;

	std::map<int,float> timers;
};


#endif // __state_h__


