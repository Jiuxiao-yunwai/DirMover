using Microsoft.Win32.SafeHandles;
using System.Buffers.Binary;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

namespace DirMover;

internal static class MftScanner
{
    private const uint GenericRead = 0x80000000;
    private const uint ShareAll = 0x00000007;
    private const uint OpenExisting = 3;
    private const uint BackupSemantics = 0x02000000;
    private const uint FsctlGetNtfsVolumeData = 0x00090064;
    private const uint FileAttributeReparsePoint = 0x00000400;
    private const uint FileAttributeDirectory = 0x00000010;
    private const uint AttributeFileName = 0x30;
    private const uint AttributeData = 0x80;
    private const uint AttributeReparsePoint = 0xC0;
    private const uint AttributeEnd = 0xFFFFFFFF;
    private const long RecordNumberMask = 0x0000FFFFFFFFFFFF;

    public static ScanSnapshot ScanChildren(
        string root, IProgress<ScanProgress> progress, CancellationToken token)
    {
        root = Path.GetFullPath(root);
        progress.Report(new ScanProgress(1, "正在打开 NTFS 卷…"));
        var drive = new DriveInfo(Path.GetPathRoot(root)!);
        if (!drive.IsReady || !drive.DriveFormat.Equals("NTFS", StringComparison.OrdinalIgnoreCase))
            throw new NotSupportedException("MFT 扫描仅支持本地 NTFS 卷。");

        var rootId = FileId(root);
        using var volume = CreateFile($@"\\.\{drive.Name[..2]}", GenericRead, ShareAll,
            IntPtr.Zero, OpenExisting, 0, IntPtr.Zero);
        if (volume.IsInvalid)
            throw new IOException("无法打开 NTFS 卷读取 MFT。", new Win32Exception(Marshal.GetLastWin32Error()));
        if (!DeviceIoControl(volume, FsctlGetNtfsVolumeData, IntPtr.Zero, 0,
                out var volumeData, Marshal.SizeOf<NtfsVolumeData>(), out _, IntPtr.Zero))
            throw new IOException("无法读取 NTFS 卷信息。", new Win32Exception(Marshal.GetLastWin32Error()));

        ValidateVolumeData(volumeData);
        var runs = ReadMftRuns(volume, volumeData);
        var directories = new Dictionary<long, DirectoryNode>();
        var detectedLinks = new List<LinkRow>();

        ReadRecords(volume, volumeData, runs, token, (id, record) =>
        {
            if (record.IsDirectory && record.Name is not null)
                directories[id] = new DirectoryNode(record.ParentId, record.Name, record.IsReparse);
        }, value => progress.Report(new ScanProgress(
            5 + (int)(value * 40), "正在读取 MFT 目录记录…")));
        token.ThrowIfCancellationRequested();
        progress.Report(new ScanProgress(47, "正在建立目录索引…"));

        var topByDirectory = new Dictionary<long, long> { [rootId] = 0 };
        var aggregates = new Dictionary<long, FolderAggregate>();
        foreach (var (id, directory) in directories)
        {
            if (directory.ParentId == rootId)
                aggregates[id] = new FolderAggregate(directory.Name, directory.IsReparse);
        }

        foreach (var (_, directory) in directories)
        {
            if (!directory.IsReparse || directory.ParentId != rootId) continue;
            var path = Path.Combine(root, directory.Name);
            detectedLinks.Add(Scanner.CreateLinkRow(path, true, root));
        }

        ReadRecords(volume, volumeData, runs, token, (_, record) =>
        {
            if (record.IsDirectory || record.Name is null) return;
            if (record.IsReparse && record.ParentId == rootId)
            {
                detectedLinks.Add(Scanner.CreateLinkRow(
                    Path.Combine(root, record.Name), false, root));
                return;
            }
            var top = ResolveTop(record.ParentId, rootId, directories, topByDirectory);
            if (top == 0 || !aggregates.TryGetValue(top, out var aggregate)) return;
            if (record.IsReparse) return;
            aggregate.LogicalBytes += record.LogicalBytes;
            aggregate.AllocatedBytes += record.AllocatedBytes;
            aggregate.Files++;
        }, value => progress.Report(new ScanProgress(
            50 + (int)(value * 45), "正在统计文件大小…")));

        var rows = new List<FolderRow>(aggregates.Count);
        foreach (var (_, aggregate) in aggregates)
        {
            token.ThrowIfCancellationRequested();
            var path = Path.Combine(root, aggregate.Name);
            if (aggregate.IsReparse)
            {
                var target = Scanner.LinkTarget(path);
                rows.Add(new FolderRow(aggregate.Name, path, 0, 0, 0, 1, 0, true,
                    target is null ? "MFT：链接（目标不可读）" : $"MFT：链接 → {target}"));
            }
            else
            {
                rows.Add(new FolderRow(aggregate.Name, path, aggregate.LogicalBytes,
                    aggregate.AllocatedBytes, aggregate.Files, 0, 0, false, "MFT 极速扫描"));
            }
        }
        progress.Report(new ScanProgress(100, "扫描完成"));
        return new ScanSnapshot(rows, detectedLinks, "NTFS MFT");
    }

    private static void ReadRecords(
        SafeFileHandle volume, NtfsVolumeData data, List<DataRun> runs,
        CancellationToken token, Action<long, ParsedRecord> accept,
        Action<double>? reportProgress = null)
    {
        var recordSize = checked((int)data.BytesPerFileRecordSegment);
        var validLength = data.MftValidDataLength;
        var chunkSize = Math.Max(recordSize, 4 * 1024 * 1024 / recordSize * recordSize);

        foreach (var run in runs.OrderBy(x => x.VirtualCluster))
        {
            if (run.LogicalCluster < 0)
                throw new IOException("$MFT 包含不支持的稀疏数据段。");
            var virtualStart = checked(run.VirtualCluster * data.BytesPerCluster);
            if (virtualStart >= validLength) break;
            var runBytes = Math.Min(checked(run.ClusterCount * data.BytesPerCluster),
                validLength - virtualStart);
            if (virtualStart % recordSize != 0)
                throw new IOException("$MFT 数据段未按文件记录对齐。");

            for (long consumed = 0; consumed < runBytes;)
            {
                token.ThrowIfCancellationRequested();
                var length = (int)Math.Min(chunkSize, runBytes - consumed);
                length -= length % recordSize;
                if (length == 0) break;
                var buffer = new byte[length];
                ReadExactly(volume, buffer, checked(run.LogicalCluster * data.BytesPerCluster + consumed));
                var firstRecord = (virtualStart + consumed) / recordSize;
                for (var offset = 0; offset + recordSize <= buffer.Length; offset += recordSize)
                {
                    var record = buffer.AsSpan(offset, recordSize);
                    if (!ApplyFixup(record, checked((int)data.BytesPerSector))) continue;
                    var parsed = ParseRecord(record);
                    if (parsed is not null)
                        accept(firstRecord + offset / recordSize, parsed.Value);
                }
                consumed += length;
                reportProgress?.Invoke(Math.Clamp(
                    (double)(virtualStart + consumed) / validLength, 0, 1));
            }
        }
    }

    private static List<DataRun> ReadMftRuns(SafeFileHandle volume, NtfsVolumeData data)
    {
        var record = new byte[checked((int)data.BytesPerFileRecordSegment)];
        ReadExactly(volume, record, checked(data.MftStartLcn * data.BytesPerCluster));
        if (!ApplyFixup(record, checked((int)data.BytesPerSector)))
            throw new IOException("无法解析 $MFT 的首条文件记录。");

        var attributesOffset = BinaryPrimitives.ReadUInt16LittleEndian(record.AsSpan(20, 2));
        for (var offset = (int)attributesOffset; offset + 24 <= record.Length;)
        {
            var type = BinaryPrimitives.ReadUInt32LittleEndian(record.AsSpan(offset, 4));
            if (type == AttributeEnd) break;
            var length = checked((int)BinaryPrimitives.ReadUInt32LittleEndian(record.AsSpan(offset + 4, 4)));
            if (length < 24 || offset + length > record.Length)
                throw new IOException("$MFT 属性损坏。");
            var nonResident = record[offset + 8] != 0;
            var nameLength = record[offset + 9];
            if (type == AttributeData && nonResident && nameLength == 0)
            {
                var firstVcn = BinaryPrimitives.ReadInt64LittleEndian(record.AsSpan(offset + 16, 8));
                var runOffset = BinaryPrimitives.ReadUInt16LittleEndian(record.AsSpan(offset + 32, 2));
                if (runOffset >= length)
                    throw new IOException("$MFT 数据段偏移无效。");
                var runs = DecodeRuns(record.AsSpan(offset + runOffset, length - runOffset), firstVcn);
                var coveredBytes = runs.Sum(x => x.ClusterCount) * data.BytesPerCluster;
                if (runs.Count == 0 || firstVcn != 0 || coveredBytes < data.MftValidDataLength)
                    throw new IOException("$MFT 使用了扩展属性列表，改用兼容扫描。");
                return runs;
            }
            offset += length;
        }
        throw new IOException("找不到 $MFT 数据段。");
    }

    internal static List<DataRun> DecodeRuns(ReadOnlySpan<byte> bytes, long firstVcn = 0)
    {
        var result = new List<DataRun>();
        long currentLcn = 0;
        long currentVcn = firstVcn;
        for (var index = 0; index < bytes.Length;)
        {
            var header = bytes[index++];
            if (header == 0) break;
            var lengthBytes = header & 0x0F;
            var offsetBytes = header >> 4;
            if (lengthBytes == 0 || lengthBytes > 8 || offsetBytes > 8 ||
                index + lengthBytes + offsetBytes > bytes.Length)
                throw new IOException("无效的 NTFS 数据段编码。");

            var clusters = ReadUnsigned(bytes.Slice(index, lengthBytes));
            index += lengthBytes;
            long lcn = -1;
            if (offsetBytes > 0)
            {
                currentLcn = checked(currentLcn + ReadSigned(bytes.Slice(index, offsetBytes)));
                lcn = currentLcn;
            }
            index += offsetBytes;
            result.Add(new DataRun(currentVcn, lcn, clusters));
            currentVcn = checked(currentVcn + clusters);
        }
        return result;
    }

    private static ParsedRecord? ParseRecord(ReadOnlySpan<byte> record)
    {
        if (!record[..4].SequenceEqual("FILE"u8)) return null;
        var flags = BinaryPrimitives.ReadUInt16LittleEndian(record.Slice(22, 2));
        if ((flags & 1) == 0) return null;

        var isDirectory = (flags & 2) != 0;
        string? name = null;
        long parent = 0;
        uint fileAttributes = 0;
        var hasReparseAttribute = false;
        var bestNamespace = -1;
        long logical = 0;
        long allocated = 0;
        var attributesOffset = BinaryPrimitives.ReadUInt16LittleEndian(record.Slice(20, 2));

        for (var offset = (int)attributesOffset; offset + 24 <= record.Length;)
        {
            var type = BinaryPrimitives.ReadUInt32LittleEndian(record.Slice(offset, 4));
            if (type == AttributeEnd) break;
            var length = checked((int)BinaryPrimitives.ReadUInt32LittleEndian(record.Slice(offset + 4, 4)));
            if (length < 24 || offset + length > record.Length) return null;
            var nonResident = record[offset + 8] != 0;
            var attributeNameLength = record[offset + 9];

            if (type == AttributeReparsePoint)
                hasReparseAttribute = true;

            if (type == AttributeFileName && !nonResident)
            {
                var valueLength = checked((int)BinaryPrimitives.ReadUInt32LittleEndian(record.Slice(offset + 16, 4)));
                var valueOffset = BinaryPrimitives.ReadUInt16LittleEndian(record.Slice(offset + 20, 2));
                var value = offset + valueOffset;
                if (valueLength >= 66 && value + valueLength <= offset + length)
                {
                    var nameLength = record[value + 64];
                    var nameSpace = record[value + 65];
                    var score = nameSpace switch { 3 => 3, 1 => 2, 0 => 1, _ => 0 };
                    if (score > bestNamespace && 66 + nameLength * 2 <= valueLength)
                    {
                        // ponytail: one canonical Win32 name avoids double-counting hard links.
                        name = Encoding.Unicode.GetString(record.Slice(value + 66, nameLength * 2));
                        parent = BinaryPrimitives.ReadInt64LittleEndian(record.Slice(value, 8)) & RecordNumberMask;
                        fileAttributes = BinaryPrimitives.ReadUInt32LittleEndian(record.Slice(value + 56, 4));
                        bestNamespace = score;
                    }
                }
            }
            else if (type == AttributeData && attributeNameLength == 0)
            {
                if (nonResident && length >= 64)
                {
                    allocated = BinaryPrimitives.ReadInt64LittleEndian(record.Slice(offset + 40, 8));
                    logical = BinaryPrimitives.ReadInt64LittleEndian(record.Slice(offset + 48, 8));
                }
                else if (!nonResident && length >= 24)
                {
                    logical = BinaryPrimitives.ReadUInt32LittleEndian(record.Slice(offset + 16, 4));
                    allocated = logical;
                }
            }
            offset += length;
        }

        isDirectory |= (fileAttributes & FileAttributeDirectory) != 0;
        return new ParsedRecord(parent, name, isDirectory,
            hasReparseAttribute || (fileAttributes & FileAttributeReparsePoint) != 0,
            Math.Max(0, logical), Math.Max(0, allocated));
    }

    internal static bool ReparseDirectorySelfTest()
    {
        var record = new byte[256];
        "FILE"u8.CopyTo(record);
        BinaryPrimitives.WriteUInt16LittleEndian(record.AsSpan(20, 2), 48);
        BinaryPrimitives.WriteUInt16LittleEndian(record.AsSpan(22, 2), 1);

        const int fileName = 48;
        BinaryPrimitives.WriteUInt32LittleEndian(record.AsSpan(fileName, 4), AttributeFileName);
        BinaryPrimitives.WriteUInt32LittleEndian(record.AsSpan(fileName + 4, 4), 96);
        BinaryPrimitives.WriteUInt32LittleEndian(record.AsSpan(fileName + 16, 4), 68);
        BinaryPrimitives.WriteUInt16LittleEndian(record.AsSpan(fileName + 20, 2), 24);
        BinaryPrimitives.WriteUInt32LittleEndian(record.AsSpan(fileName + 24 + 56, 4),
            FileAttributeDirectory);
        record[fileName + 24 + 64] = 1;
        record[fileName + 24 + 65] = 1;
        Encoding.Unicode.GetBytes("x").CopyTo(record.AsSpan(fileName + 24 + 66));

        const int reparse = fileName + 96;
        BinaryPrimitives.WriteUInt32LittleEndian(record.AsSpan(reparse, 4), AttributeReparsePoint);
        BinaryPrimitives.WriteUInt32LittleEndian(record.AsSpan(reparse + 4, 4), 24);
        BinaryPrimitives.WriteUInt32LittleEndian(record.AsSpan(reparse + 24, 4), AttributeEnd);

        return ParseRecord(record) is { IsDirectory: true, IsReparse: true };
    }

    private static bool ApplyFixup(Span<byte> record, int bytesPerSector)
    {
        if (record.Length < 48 || !record[..4].SequenceEqual("FILE"u8)) return false;
        var fixupOffset = BinaryPrimitives.ReadUInt16LittleEndian(record.Slice(4, 2));
        var fixupCount = BinaryPrimitives.ReadUInt16LittleEndian(record.Slice(6, 2));
        if (fixupCount < 2 || fixupOffset + fixupCount * 2 > record.Length) return false;
        var marker = BinaryPrimitives.ReadUInt16LittleEndian(record.Slice(fixupOffset, 2));
        for (var i = 1; i < fixupCount; i++)
        {
            var sectorEnd = i * bytesPerSector - 2;
            if (sectorEnd < 0 || sectorEnd + 2 > record.Length ||
                BinaryPrimitives.ReadUInt16LittleEndian(record.Slice(sectorEnd, 2)) != marker)
                return false;
            record.Slice(fixupOffset + i * 2, 2).CopyTo(record[sectorEnd..]);
        }
        return true;
    }

    private static long ResolveTop(long directoryId, long rootId,
        Dictionary<long, DirectoryNode> directories, Dictionary<long, long> cache)
    {
        if (directoryId == rootId) return 0;
        if (cache.TryGetValue(directoryId, out var known)) return known;
        var path = new List<long>();
        var current = directoryId;
        long top = 0;
        while (current != rootId && !cache.TryGetValue(current, out top))
        {
            if (!directories.TryGetValue(current, out var directory) || path.Count > 2048)
                return 0;
            path.Add(current);
            current = directory.ParentId;
        }
        if (current == rootId && path.Count > 0)
            top = path[^1];
        foreach (var id in path)
            cache[id] = top;
        return top;
    }

    private static long FileId(string path)
    {
        using var handle = CreateFile(path, 0, ShareAll, IntPtr.Zero, OpenExisting,
            BackupSemantics, IntPtr.Zero);
        if (handle.IsInvalid || !GetFileInformationByHandle(handle, out var info))
            throw new IOException("无法取得扫描目录的 NTFS 文件 ID。",
                new Win32Exception(Marshal.GetLastWin32Error()));
        return ((long)info.FileIndexHigh << 32 | info.FileIndexLow) & RecordNumberMask;
    }

    private static void ValidateVolumeData(NtfsVolumeData data)
    {
        if (data.BytesPerSector is < 512 or > 65536 ||
            data.BytesPerCluster is < 512 or > 2 * 1024 * 1024 ||
            data.BytesPerFileRecordSegment is < 512 or > 65536 ||
            data.MftValidDataLength <= 0)
            throw new IOException("NTFS 卷参数异常。");
    }

    private static void ReadExactly(SafeFileHandle handle, byte[] buffer, long offset)
    {
        var read = 0;
        while (read < buffer.Length)
        {
            var count = RandomAccess.Read(handle, buffer.AsSpan(read), offset + read);
            if (count == 0) throw new EndOfStreamException("读取 NTFS 卷时提前结束。");
            read += count;
        }
    }

    private static long ReadUnsigned(ReadOnlySpan<byte> value)
    {
        long result = 0;
        for (var i = 0; i < value.Length; i++)
            result |= (long)value[i] << (i * 8);
        return result;
    }

    private static long ReadSigned(ReadOnlySpan<byte> value)
    {
        var result = ReadUnsigned(value);
        if (value.Length < 8 && (value[^1] & 0x80) != 0)
            result |= -1L << (value.Length * 8);
        return result;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern SafeFileHandle CreateFile(
        string name, uint access, uint share, IntPtr security, uint creation,
        uint flags, IntPtr template);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DeviceIoControl(
        SafeFileHandle device, uint controlCode, IntPtr input, int inputSize,
        out NtfsVolumeData output, int outputSize, out int bytesReturned, IntPtr overlapped);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileInformationByHandle(
        SafeFileHandle file, out ByHandleFileInformation information);

    [StructLayout(LayoutKind.Sequential)]
    private struct NtfsVolumeData
    {
        public long VolumeSerialNumber;
        public long NumberSectors;
        public long TotalClusters;
        public long FreeClusters;
        public long TotalReserved;
        public uint BytesPerSector;
        public uint BytesPerCluster;
        public uint BytesPerFileRecordSegment;
        public uint ClustersPerFileRecordSegment;
        public long MftValidDataLength;
        public long MftStartLcn;
        public long Mft2StartLcn;
        public long MftZoneStart;
        public long MftZoneEnd;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ByHandleFileInformation
    {
        public uint FileAttributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME CreationTime;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastAccessTime;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWriteTime;
        public uint VolumeSerialNumber;
        public uint FileSizeHigh;
        public uint FileSizeLow;
        public uint NumberOfLinks;
        public uint FileIndexHigh;
        public uint FileIndexLow;
    }

    internal readonly record struct DataRun(long VirtualCluster, long LogicalCluster, long ClusterCount);
    private readonly record struct ParsedRecord(
        long ParentId, string? Name, bool IsDirectory, bool IsReparse,
        long LogicalBytes, long AllocatedBytes);
    private readonly record struct DirectoryNode(long ParentId, string Name, bool IsReparse);

    private sealed class FolderAggregate(string name, bool isReparse)
    {
        public string Name { get; } = name;
        public bool IsReparse { get; } = isReparse;
        public long LogicalBytes;
        public long AllocatedBytes;
        public long Files;
    }
}
