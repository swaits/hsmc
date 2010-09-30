# flags and libs
DEFS =
LIBS =

# compiler and flags
CC = cc
CCFLAGS = -Wall -O3 -c
CXX = c++
CXXFLAGS = $(CCFLAGS)
CPPFLAGS =

# linker
LD = c++
LDFLAGS =
STRIP = strip

# misc programs
RM = rm -f
INSTALL = install
MKDEPEND = mkdepend
LEX = flex -f
YACC = yacc

# our target and source code
TARGET = hsmc
SRCS = hsm-lexer.c hsm-parser.tab.c getopt.cpp main.cpp
OBJS = hsm-lexer.o hsm-parser.tab.o getopt.o main.o

##
##

all: $(TARGET)

$(TARGET): $(OBJS)
	$(LD) $(LDFLAGS) $(LIBS) -o $@ $(OBJS)

hsm-parser.tab.h: \
	hsm.y

hsm-parser.tab.c: hsm.y
	$(YACC) -d -bhsm-parser hsm.y

hsm-lexer.h: \
	hsm.l

hsm-lexer.c: hsm.l
	$(LEX) -ohsm-lexer.c hsm.l

depend:
	$(MKDEPEND) $(SRCS)

clean:
	$(RM) $(OBJS) $(TARGET)

.cpp.o:
	$(CXX) $(CXXFLAGS) $(CPPFLAGS) $(DEFS) $< -o $@

.c.o:
	$(CC) $(CCFLAGS) $(CPPFLAGS) $(DEFS) $< -o $@


# DO NOT DELETE THIS LINE -- make  depend  depends  on it.

hsm-lexer.o: \
	hsm-parser.tab.h 

hsm-parser.o: \
	main.h 

main.o: \
	autofile.h \
	config.h \
	getopt.h \
	machine.h \
	main.h \
	state.h 

