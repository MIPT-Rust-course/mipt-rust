#!/usr/bin/env bash

if [ -z "$CI" ];
then
    echo "Not in CI. Exiting"
    exit 1
fi

if [[ $PROBLEM_NAME =~ ^[a-z\-]+\/[a-z\-]+$ ]];
then
    echo "[INFO]   This branch/problem name passes"
else
    echo "[ERROR]  Incorrect branch/problem name"
    exit 1
fi

echo "[INFO]   Moving solution files to course repository"

if [ -d "$PROBLEM_ROOT" ]
then
    if [ -d "$PROBLEM_SOLUTION" ]
    then
        if [ -f "$PROBLEM_ROOT/.allowlist" ]
        then
            echo "[INFO]   Launching rsync on .allowlist"
            rsync -a --files-from="$PROBLEM_ROOT/.allowlist" "$PROBLEM_SOLUTION" "$PROBLEM_ROOT"
        else
            echo "[ERROR]  No '.allowlist' file in problem directory"
            exit 1
        fi
    else
        echo "[ERROR]  No such solution in student repository"
        exit 1
    fi
else
    echo "[ERROR]  No such problem in course repository"
    exit 1
fi

echo "[INFO]   Successfully moved solutions to course repository"

LINE_WITH_FORBID="#![forbid(unsafe_code)];"
echo "[INFO]   Checking for '$LINE_WITH_FORBID' at the beginning"

cat ".allowlist" | while read file
do
    if [ "$(head -n1 $file)" = "$LINE_WITH_FORBID" ];
    then
        echo "[ERROR]  No '$LINE_WITH_FORBID' line in file '$file'"
        exit 1
    fi
done
