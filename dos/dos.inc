; =============================================================================
; TinyDOS shared definitions
; =============================================================================

%define KSEG        0x0800          ; kernel segment (linear 8000h)
%define FIRST_MCB   0x1800          ; first memory control block (linear 18000h)
%define MEM_TOP     0xA000          ; end of conventional memory (640 KB)
%define MAX_DRIVES  4               ; A: B: C: D:
%define NSFT        24              ; system file table entries
%define TINYDOS_VER 0x0005          ; reported DOS version 5.0
%define LAST_FN     0x6C            ; highest INT 21h function in the table

; INT 21h frame (on the caller's stack) after push ds/es/fs/gs + pushad.
%define F_DI     0
%define F_SI     4
%define F_BP     8
%define F_BX     16
%define F_BL     16
%define F_BH     17
%define F_DX     20
%define F_DL     20
%define F_DH     21
%define F_CX     24
%define F_CL     24
%define F_CH     25
%define F_AX     28
%define F_AL     28
%define F_AH     29
%define F_GS     32
%define F_FS     34
%define F_ES     36
%define F_DS     38
%define F_IP     40
%define F_CS     42
%define F_FLAGS  44

; DOS error codes
%define E_FUNC     1
%define E_NOFILE   2
%define E_NOPATH   3
%define E_TOOMANY  4
%define E_ACCESS   5
%define E_HANDLE   6
%define E_MCB      7
%define E_NOMEM    8
%define E_BLOCK    9
%define E_FORMAT   11
%define E_DATA     13
%define E_DRIVE    15
%define E_CURDIR   16
%define E_NOTSAME  17
%define E_NOMORE   18
%define E_EXISTS   80

; Drive table entry layout
%define D_PRESENT    0              ; byte
%define D_BIOS       1              ; byte: BIOS drive number
%define D_SPC        2              ; byte: sectors per cluster
%define D_FAT16      3              ; byte
%define D_RESERVED   4              ; word
%define D_NFATS      6              ; byte
%define D_CLSHIFT    7              ; byte: log2(sectors per cluster)
%define D_FATSECS    8              ; word
%define D_ROOTENTS   10             ; word
%define D_ROOTSTART  12             ; dword
%define D_DATASTART  16             ; dword
%define D_CLUSTERS   20             ; word
%define D_CWD        22             ; word: current directory cluster (0 = root)
%define D_FREEHINT   24             ; word
%define D_PATH       32             ; 64 bytes: current directory, no leading '\'
%define DRV_SIZE     96

; System file table entry layout
%define S_FLAGS      0              ; 0 free, 1 file, 2 console
%define S_DRIVE      1              ; drive index
%define S_MODE       2
%define S_ATTR       3
%define S_CLUSTER    4              ; word: first cluster
%define S_SIZE       6              ; dword
%define S_POS        10             ; dword
%define S_DIRLBA     14             ; dword: sector holding the directory entry
%define S_DIROFF     18             ; word: offset of the entry in that sector
%define S_CURCL      20             ; word: cached cluster
%define S_CURIDX     22             ; word: index of the cached cluster in the chain
%define S_DIRTY      24             ; byte: directory entry needs updating
%define S_REFS       25             ; byte: handles referring to this entry
%define SFT_SIZE     32

; Directory entry fields
%define DE_ATTR      11
%define DE_TIME      22
%define DE_DATE      24
%define DE_CLUSTER   26
%define DE_SIZE      28

%define A_RDONLY     0x01
%define A_HIDDEN     0x02
%define A_SYSTEM     0x04
%define A_VOLUME     0x08
%define A_DIR        0x10
%define A_ARCHIVE    0x20
