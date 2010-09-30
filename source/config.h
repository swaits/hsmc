#ifndef __config_h__
#define __config_h__

#include <string>
#include <cstdlib>
#include "getopt.h"

class Config
{
public:

	std::string InFileName;
	std::string OutCPPFileName;
	std::string OutHFileName;
	std::string IncludeName;
	bool        Debug;
	bool        Quiet;
	bool        PureVirtuals;

	void Usage()
	{
		printf(
					"\n"
					"\nHSMC - Hierarchical State Machine Compiler\n"
					"\n"
					"Usage: hsmc [-hdpq] [-o prefix] source_file\n"
					"\n"
					"  -h            this help\n"
					"  -d            include debugging information\n"
					"  -p            actions declared as pure virtual\n"
					"  -q            quiet mode (only output errors)\n"
					"  -o prefix     specify prefix for output files [prefix.cpp,prefix.h]\n"
					"  source_file   HSM source file\n"
					"\n"
					);
	}

	bool ParseCommandLine(int argc, char* argv[])
	{
		// reset defaults
		InFileName     = "";
		OutCPPFileName = "";
		OutHFileName   = "";
		IncludeName    = "";
		Debug          = false;
		Quiet          = false;
		PureVirtuals   = false;

		// temporary
		std::string prefix = "";

		// parse command line
		int ch;
		while ( (ch = getopt(argc,argv,"hdpqo:")) != -1 )
		{
			switch ( ch )
			{
				case 'p':
					PureVirtuals = true;
					break;

				case 'q':
					Quiet = true;
					break;

				case 'd':
					Debug = true;
					break;

				case 'o':
					prefix = optarg;
					break;

				case 'h':
				default:
					Usage();
					return false;
			}
		}

		// check for input file
		if ( (argc-optind) == 0 )
		{
			printf("ERROR: no input file specified\n");
			Usage();
			return false;
		}
		else if ( (argc-optind) > 1 )
		{
			printf("ERROR: multiple input files not allowed\n");
			Usage();
			return false;
		}

		// got input file
		InFileName = argv[optind];

		// determine output prefix if it wasn't specified
		if ( prefix == "" )
		{
			// prefix NOT specified, let's get it from the base
			prefix = InFileName.substr(0,InFileName.rfind('.'));
		}

		// determine output files specified
		OutCPPFileName = prefix + ".cpp";
		OutHFileName   = prefix + ".h";

		// strip up to any '/' or '\'
		IncludeName = OutHFileName;
		if ( OutHFileName.rfind('/') >= 0 )
		{
			IncludeName = IncludeName.substr( IncludeName.rfind('/')+1 );
		}
		if ( OutHFileName.rfind('\\') >= 0 )
		{
			IncludeName = IncludeName.substr( IncludeName.rfind('\\')+1 );
		}

		// success
		return true;
	}
};


#endif // __config_h__

