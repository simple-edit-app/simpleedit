#!/bin/sh
# Asks for what git or ssh need (a password, a passphrase, a user name) in a
# dialog of the system: SimpleEdit has no terminal to ask in.
# The prompt reaches the dialog as an argument, never through a shell.
prompt="$1"
hidden=0
case "$prompt" in
    *[Pp]assword*|*[Pp]assphrase*|*PIN*) hidden=1 ;;
esac
if [ "$(uname)" = "Darwin" ]; then
    if [ "$hidden" = 1 ]; then
        exec osascript \
            -e 'on run argv' \
            -e 'text returned of (display dialog (item 1 of argv) default answer "" with hidden answer with title "SimpleEdit - Git")' \
            -e 'end run' "$prompt"
    fi
    exec osascript \
        -e 'on run argv' \
        -e 'text returned of (display dialog (item 1 of argv) default answer "" with title "SimpleEdit - Git")' \
        -e 'end run' "$prompt"
fi
if command -v zenity >/dev/null 2>&1; then
    if [ "$hidden" = 1 ]; then
        exec zenity --entry --hide-text --title="SimpleEdit - Git" --text="$prompt"
    fi
    exec zenity --entry --title="SimpleEdit - Git" --text="$prompt"
fi
if command -v kdialog >/dev/null 2>&1; then
    if [ "$hidden" = 1 ]; then
        exec kdialog --title "SimpleEdit - Git" --password "$prompt"
    fi
    exec kdialog --title "SimpleEdit - Git" --inputbox "$prompt"
fi
exit 1
