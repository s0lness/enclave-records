#!/bin/bash
pkill -f "speculos.*--api-port 500[12]" 2>/dev/null
echo "emulators stopped"
