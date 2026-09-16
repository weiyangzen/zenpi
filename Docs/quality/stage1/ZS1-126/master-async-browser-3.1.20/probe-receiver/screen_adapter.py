"""Exact screen_text extracted from fully read upstream fixture adapter."""
import re
import unicodedata

def screen_text(data,width=140,height=40,left_only=False):
    """Reconstruct the cursor-addressed cells Ratatui writes over a real PTY.

    Support CSI cursor/erase and OSC controls, including wide Unicode cells;
    styles do not change text. This is an assertion aid, not a replacement UI.
    """
    rows=[[' ']*width for _ in range(height)];x=y=0;i=0
    text=data.decode('utf-8',errors='replace')
    while i<len(text):
        char=text[i]
        if char=='\x1b':
            match=re.match(r'\x1b\[([0-?]*)([ -/]*)([@-~])',text[i:])
            if match:
                raw,_,command=match.groups();i+=len(match.group());parts=raw.lstrip('?').split(';');values=[int(p) if p.isdigit() else 0 for p in parts];n=values[0] or 1
                if command in 'Hf':y=max(0,min(height-1,n-1));x=max(0,min(width-1,(values[1] if len(values)>1 and values[1] else 1)-1))
                elif command=='G':x=max(0,min(width-1,n-1))
                elif command=='d':y=max(0,min(height-1,n-1))
                elif command=='A':y=max(0,y-n)
                elif command=='B':y=min(height-1,y+n)
                elif command=='C':x=min(width-1,x+n)
                elif command=='D':x=max(0,x-n)
                elif command=='J' and values[0] in (2,3):rows=[[' ']*width for _ in range(height)]
                elif command=='K':
                    start,end=(0,width) if values[0]==2 else ((0,x+1) if values[0]==1 else (x,width));rows[y][start:end]=[' ']*(end-start)
                elif command=='h' and raw=='?1049':rows=[[' ']*width for _ in range(height)];x=y=0
                continue
            match=re.match(r'\x1b\][^\a]*(?:\a|\x1b\\)',text[i:])
            if match:i+=len(match.group());continue
            i+=2;continue
        if char=='\r':x=0
        elif char=='\n':y=min(height-1,y+1)
        elif char=='\b':x=max(0,x-1)
        elif ord(char)>=32:
            cells=0 if unicodedata.combining(char) else (2 if unicodedata.east_asian_width(char) in ('W','F') else 1)
            if cells==0 and x>0:rows[y][x-1]+=char
            elif cells and x<width:
                rows[y][x]=char
                if cells==2 and x+1<width:rows[y][x+1]=''
                x+=cells
        i+=1
    return '\n'.join(''.join(row[:56] if left_only else row) for row in rows).encode()
