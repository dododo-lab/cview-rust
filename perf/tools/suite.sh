#!/bin/zsh
# suite.sh <tag> <app-bundle> — 같은 채널로 a~f 전부
T=/Volumes/data/project/Rust/CView/target/perf-tools; tag=$1
rm -f $T/$tag.jsonl
$T/launch.sh $2 >/dev/null
C1='["b628d1039a84ecc703804e17acee2eb3"]'; C2='["65c3035bdc598c81f15a8fe0e958b3ce"]'
C4='["65c3035bdc598c81f15a8fe0e958b3ce","e14259667937be6e561d28ab156718be","6bd0bb97d31365e7834d8113bb01d889","55e243bd868e55adf3524c85f8db51b5"]'
$T/run.sh $tag e $C4; $T/run.sh $tag a; $T/run.sh $tag b; $T/run.sh $tag c $C1; $T/run.sh $tag d $C2; $T/run.sh $tag f $C1
