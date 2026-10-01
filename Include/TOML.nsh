; TOML.nsh — loops over TOML arrays and tables.
;
;   ${TomlForEach} "cfg" "server" $0 $1
;       DetailPrint "$0 = $1"
;   ${TomlNext}
;
; Each pass sets the key and the value; for an array the key is the index.
; ${TomlBreak} leaves the loop and ${Continue} skips to the next entry. A
; path that does not exist, or is not an array or table, runs zero passes.
;
; Loops nest: each one keeps its counter in its own Var /GLOBAL, named from
; ${__COUNTER__}, and a define stack (the pattern of LogicLib's _PushScope)
; tracks which loop is innermost.

!ifndef TOML_NSH
!define TOML_NSH

!include "LogicLib.nsh"

!macro _TomlPushScope id
	!ifdef _TomlId
		!define _TomlPrev${id} ${_TomlId}
		!undef _TomlId
	!endif
	!define _TomlId ${id}
!macroend

!macro _TomlPopScope
	!ifndef _TomlId
		!error "${TomlNext} without a ${TomlForEach}"
	!endif
	!ifdef _TomlPrev${_TomlId}
		!define _TomlCur ${_TomlId}
		!undef _TomlId
		!define _TomlId ${_TomlPrev${_TomlCur}}
		!undef _TomlPrev${_TomlCur}
		!undef _TomlCur
	!else
		!undef _TomlId
	!endif
!macroend

!macro TomlForEach name path key value
	!insertmacro _TomlPushScope ${__COUNTER__}
	Var /GLOBAL _TomlI${_TomlId}
	Var /GLOBAL _TomlN${_TomlId}
	StrCpy $_TomlN${_TomlId} 0
	ClearErrors
	TOML::Count "${name}" "${path}"
	${IfNot} ${Errors}
		Pop $_TomlN${_TomlId}
	${EndIf}
	StrCpy $_TomlI${_TomlId} 0
	${DoWhile} $_TomlI${_TomlId} < $_TomlN${_TomlId}
		TOML::EntryAt "${name}" "${path}" $_TomlI${_TomlId}
		Pop ${key}
		Pop ${value}
		IntOp $_TomlI${_TomlId} $_TomlI${_TomlId} + 1
!macroend
!define TomlForEach "!insertmacro TomlForEach"

!macro TomlNext
	${Loop}
	!insertmacro _TomlPopScope
!macroend
!define TomlNext "!insertmacro TomlNext"

!define TomlBreak "${ExitDo}"

!endif
