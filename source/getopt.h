#ifndef __getopt_h__
#define __getopt_h__

extern int  opterr;   /* if error message should be printed */
extern int  optind;   /* index into parent argv vector */
extern int  optopt;   /* character checked for validity */
extern int  optreset; /* reset getopt */
extern char *optarg;  /* argument associated with option */

#if defined(__cplusplus)
extern "C" {
#endif

int getopt(int nargc, char* const* nargv, const char* ostr);

#if defined(__cplusplus)
}
#endif

#endif // __getopt_h__
