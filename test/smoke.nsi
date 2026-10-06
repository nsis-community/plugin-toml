; Smoke test for the TOML plug-in.
;
; Built by `mise run smoke`, which supplies PLUGINDIR and OUTFILE and picks
; the variant with -XTarget. Results go to smoke.log next to the installer;
; the installer itself is silent.

Unicode true

!include "LogicLib.nsh"
!include "${__FILEDIR__}\..\Include\TOML.nsh"

!ifndef PLUGINDIR
	!error "define PLUGINDIR: the Plugins/<variant> directory holding TOML.dll"
!endif
!ifndef OUTFILE
	!define OUTFILE "smoke.exe"
!endif

!addplugindir "${PLUGINDIR}"

Name "TOML plug-in smoke test"
OutFile "${OUTFILE}"
RequestExecutionLevel user
SilentInstall silent
ShowInstDetails nevershow

Var LOG
Var FAILURES
Var FILE

!macro Expect name expected
	${If} $0 == "${expected}"
		FileWrite $LOG "ok   ${name}: $0$\r$\n"
	${Else}
		IntOp $FAILURES $FAILURES + 1
		FileWrite $LOG "FAIL ${name}: expected '${expected}', got '$0'$\r$\n"
	${EndIf}
!macroend

!macro ExpectError name
	${If} ${Errors}
		TOML::LastError
		Pop $0
		FileWrite $LOG "ok   ${name}: error flag set ($0)$\r$\n"
	${Else}
		IntOp $FAILURES $FAILURES + 1
		FileWrite $LOG "FAIL ${name}: error flag not set$\r$\n"
	${EndIf}
!macroend

!macro ExpectNoError name
	${If} ${Errors}
		IntOp $FAILURES $FAILURES + 1
		TOML::LastError
		Pop $0
		FileWrite $LOG "FAIL ${name}: error flag set ($0)$\r$\n"
	${Else}
		FileWrite $LOG "ok   ${name}: no error$\r$\n"
	${EndIf}
!macroend

Section "Smoke"
	StrCpy $FAILURES 0
	FileOpen $LOG "$EXEDIR\smoke.log" w
	InitPluginsDir

	FileOpen $FILE "$PLUGINSDIR\in.toml" w
	FileWrite $FILE "# comment$\n"
	FileWrite $FILE "title = $\"smoke$\"$\n"
	FileWrite $FILE "[server]$\n"
	FileWrite $FILE "host = $\"localhost$\" # keep$\n"
	FileWrite $FILE "port = 80$\n"
	FileWrite $FILE "[[plugins]]$\n"
	FileWrite $FILE "name = $\"a$\"$\n"
	FileWrite $FILE "[[plugins]]$\n"
	FileWrite $FILE "name = $\"b$\"$\n"
	FileClose $FILE

	; Reading.
	ClearErrors
	TOML::Load "cfg" "$PLUGINSDIR\in.toml"
	!insertmacro ExpectNoError "Load"

	TOML::Get "cfg" "title"
	Pop $0
	!insertmacro Expect "Get/string" "smoke"

	TOML::Get "cfg" "server.port"
	Pop $0
	!insertmacro Expect "Get/integer" "80"

	TOML::Get "cfg" "plugins[1].name"
	Pop $0
	!insertmacro Expect "Get/index" "b"

	TOML::Type "cfg" "server"
	Pop $0
	!insertmacro Expect "Type" "table"

	TOML::Count "cfg" "plugins"
	Pop $0
	!insertmacro Expect "Count" "2"

	; Argument order and push order through the real stack.
	TOML::EntryAt "cfg" "server" 1
	Pop $1
	Pop $2
	StrCpy $0 "$1=$2"
	!insertmacro Expect "EntryAt" "port=80"

	; Iteration.
	StrCpy $0 ""
	${TomlForEach} "cfg" "server" $1 $2
		StrCpy $0 "$0$1=$2;"
	${TomlNext}
	!insertmacro Expect "ForEach" "host=localhost;port=80;"

	StrCpy $0 ""
	${TomlForEach} "cfg" "plugins" $1 $2
		${TomlForEach} "cfg" "plugins[$1]" $3 $4
			StrCpy $0 "$0$1.$3=$4;"
		${TomlNext}
	${TomlNext}
	!insertmacro Expect "ForEach/nested" "0.name=a;1.name=b;"

	StrCpy $0 ""
	${TomlForEach} "cfg" "" $1 $2
		StrCpy $0 "$0$1;"
		${If} $1 == "server"
			${TomlBreak}
		${EndIf}
	${TomlNext}
	!insertmacro Expect "ForEach/break" "title;server;"

	StrCpy $0 "untouched"
	${TomlForEach} "cfg" "missing" $1 $2
		StrCpy $0 "ran"
	${TomlNext}
	!insertmacro Expect "ForEach/missing" "untouched"

	; Writing, then reading the file back.
	ClearErrors
	TOML::SetInt "cfg" "server.port" "0x1F90"
	TOML::SetString "cfg" "server.host" "example.com"
	TOML::SetBool "cfg" "server.tls" 1
	TOML::SetString "cfg" "plugins[2].name" "c"
	TOML::SetString "cfg" "new.nested.key" "created"
	TOML::Save "cfg" "$PLUGINSDIR\out.toml"
	TOML::Free "cfg"
	TOML::Load "cfg" "$PLUGINSDIR\out.toml"
	!insertmacro ExpectNoError "Set/Save/Load"

	TOML::Get "cfg" "server.port"
	Pop $0
	!insertmacro Expect "SetInt" "8080"
	TOML::Get "cfg" "server.tls"
	Pop $0
	!insertmacro Expect "SetBool" "true"
	TOML::Get "cfg" "plugins[2].name"
	Pop $0
	!insertmacro Expect "Set/append" "c"
	TOML::Get "cfg" "new.nested.key"
	Pop $0
	!insertmacro Expect "Set/create" "created"

	FileOpen $FILE "$PLUGINSDIR\out.toml" r
	FileRead $FILE $0
	FileRead $FILE $0
	FileRead $FILE $0
	FileRead $FILE $0
	FileClose $FILE
	!insertmacro Expect "Save/fidelity" "host = $\"example.com$\" # keep$\n"

	; Failures set the flag and push nothing.
	Push "sentinel"
	ClearErrors
	TOML::Get "cfg" "no.such.key"
	!insertmacro ExpectError "Get/missing"
	ClearErrors
	TOML::Load "other" "$PLUGINSDIR\does-not-exist.toml"
	!insertmacro ExpectError "Load/missing"
	ClearErrors
	TOML::SetBool "cfg" "x" "maybe"
	!insertmacro ExpectError "SetBool/invalid"
	Pop $0
	!insertmacro Expect "Failure/stack" "sentinel"

	; The longest value this installer can hold round-trips. Against a stock
	; makensis that is 1023 characters, against /DNSIS_MAX_STRLEN=8192 it is
	; 8191, with the same DLL.
	!define /math MAX ${NSIS_MAX_STRLEN} - 1
	StrCpy $1 ""
	StrCpy $2 0
	${Do}
		StrCpy $1 "$1abcdefghij"
		IntOp $2 $2 + 10
	${LoopUntil} $2 >= ${MAX}
	StrCpy $1 $1 ${MAX}
	ClearErrors
	TOML::SetString "cfg" "long" $1
	TOML::Get "cfg" "long"
	Pop $0
	!insertmacro ExpectNoError "Get/max"
	StrLen $3 $0
	StrCpy $0 $3
	!insertmacro Expect "Get/max length" "${MAX}"

	; One character more than that must come back truncated, with the flag.
	FileOpen $FILE "$PLUGINSDIR\long.toml" w
	; In pieces: the whole line would not fit in an NSIS string either.
	FileWrite $FILE "longer = $\"x"
	FileWrite $FILE $1
	FileWrite $FILE "$\"$\n"
	FileClose $FILE
	TOML::Load "cfg" "$PLUGINSDIR\long.toml"
	ClearErrors
	TOML::Get "cfg" "longer"
	Pop $4
	!insertmacro ExpectError "Get/truncated"
	StrLen $0 $4
	!insertmacro Expect "Get/truncated length" "${MAX}"

	TOML::Free "cfg"
	FileWrite $LOG "note NSIS_MAX_STRLEN is ${NSIS_MAX_STRLEN}$\r$\n"

	${If} $FAILURES == 0
		FileWrite $LOG "ALL PASSED$\r$\n"
	${Else}
		FileWrite $LOG "$FAILURES FAILED$\r$\n"
	${EndIf}
	FileClose $LOG

	SetErrorLevel $FAILURES
SectionEnd
