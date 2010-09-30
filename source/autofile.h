#ifndef __autofile_h__
#define __autofile_h__

#include <cstdio>

class AutoFILE
{
public:
	AutoFILE(char* filename, char* mode = "r")
	{
		f = fopen(filename,mode);
	}
	~AutoFILE()
	{
		if ( Opened() )
			fclose(f);
	}
	operator FILE* ()
	{
		return f;
	}
	bool Opened()
	{
		return f != (FILE*)0;
	}

private:
	FILE* f;
};





#endif // __autofile_h__

