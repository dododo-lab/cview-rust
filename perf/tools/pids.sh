#!/bin/zsh
app=$(pgrep -f "MacOS/cview-app" | head -1); after=$(pgrep -f "com.apple.WebKit" | sort)
new=$(comm -13 <(cat /tmp/cview-webkit-before) <(echo $after) | tr '\n' ' ')
echo "$app $new" > /tmp/cview-pids; cat /tmp/cview-pids
