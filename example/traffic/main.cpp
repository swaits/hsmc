#include "traffic.h"
#include <stdio.h>
#include <stdlib.h>

#include <windows.h> // for Sleep()
#include <conio.h> // for kbhit()


class MyStoplight: public stoplight
{
public:
	virtual void DoPOST()
	{
		printf("DoPOST\n");
	}
	virtual void CheckForError()
	{
		if ( kbhit() )
		{
			printf("Encountered critical error\n");
			HSMTrigger(FAILURE);
		}
	}
	virtual void GreenOff ()
	{
		printf("Green Off\n");
	}
	virtual void GreenOn()
	{
		printf("Green On\n");
	}
	virtual void YellowOff()
	{
		printf("Yellow Off\n");
	}
	virtual void YellowOn()
	{
		printf("Yellow On\n");
	}
	virtual void RedOff()
	{
		printf("Red Off\n");
	}
	virtual void RedOn()
	{
		printf("Red On\n");
	}

	virtual void EnableRedFlashMode()
	{
		printf("Setting Red Flash mode\n");
	}

	virtual void HSMDebug(char* msg)
	{
		printf("HSMDebug: %s\n",msg);
	}
};

int main()
{
	printf("Press any key to simulate Error.\n");

	MyStoplight s;
	s.HSMConstruct();
	while ( s.HSMUpdate(0.1f) )
	{
		Sleep(100);
	}
	return 0;
}

