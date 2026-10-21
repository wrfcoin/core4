#!/usr/bin/env python3
"""Emit unchanged OFF corpus and the registered ON companion sequentially."""
import argparse,subprocess
from pathlib import Path
from node_share_matrix import ROOT,run

def main():
    p=argparse.ArgumentParser();p.add_argument('--out',type=Path,required=True);p.add_argument('--profile',choices=['dev','release'],default='dev');p.add_argument('--target');p.add_argument('--cross',action='store_true');args=p.parse_args()
    command=['cross' if args.cross else 'cargo','run','--locked','-p','test_server','--example','emit_roots']
    if args.profile=='release':command+=['--release']
    if args.target:command+=['--target',args.target]
    command+=['--','--out',str(args.out)]
    subprocess.run(command,cwd=ROOT,check=True)
    subprocess.run(['python3', 'scripts/test_node_share_operator.py'], cwd=ROOT, check=True)
    run(args.out.with_name('node-share-'+args.out.name),args.profile,args.target,args.cross)
if __name__=='__main__':main()
