local LrTasks = import 'LrTasks'
local LrApplication = import 'LrApplication'
local LrExportSession = import 'LrExportSession'
local LrFileUtils = import 'LrFileUtils'
local LrPathUtils = import 'LrPathUtils'

local JOB = [[D:\color-adjustment-software\.tmp\lr-export-job.lua]]
local LOG = [[D:\color-adjustment-software\.tmp\lr-reference-export.log]]

local function log(message)
    local file = io.open(LOG, 'a')
    if file then
        file:write(os.date('%Y-%m-%d %H:%M:%S') .. ' ' .. tostring(message) .. '\n')
        file:close()
    end
end

local function file_exists(path)
    local file = io.open(path, 'r')
    if file then
        file:close()
        return true
    end
    return false
end

local function copy_table(value)
    if type(value) ~= 'table' then
        return value
    end
    local out = {}
    for key, item in pairs(value) do
        out[key] = copy_table(item)
    end
    return out
end

local function load_job()
    local chunk, err = loadfile(JOB)
    if not chunk then
        return nil, err
    end
    local ok, job = pcall(chunk)
    if not ok then
        return nil, job
    end
    return job
end

local function export_settings(destination)
    return {
        LR_format = 'JPEG',
        LR_jpeg_quality = 1,
        LR_export_colorSpace = 'sRGB',
        LR_export_destinationType = 'specificFolder',
        LR_export_destinationPathPrefix = destination,
        LR_export_useSubfolder = false,
        LR_export_useParentFolder = false,
        LR_outputSharpeningOn = false,
        LR_size_doConstrain = false,
        LR_tokens = '{{image_name}}',
        LR_extensionCase = 'lowercase',
        LR_collisionHandling = 'overwrite',
        LR_includeVideoFiles = false,
        LR_minimizeEmbeddedMetadata = true,
    }
end

local function export_photo(photo, destination)
    LrFileUtils.createAllDirectories(destination)
    local session = LrExportSession {
        photosToExport = { photo },
        exportSettings = export_settings(destination),
    }
    session:doExportOnCurrentTask()
end

local function run_job(job)
    local catalog = LrApplication.activeCatalog()
    local active = catalog:getPath()
    log('active catalog: ' .. tostring(active))
    if job.expected_catalog and active ~= job.expected_catalog then
        log('catalog mismatch, aborting')
        return
    end

    for _, image in ipairs(job.images or {}) do
        local photo = catalog:findPhotoByPath(image.path)
        if not photo then
            log('missing photo: ' .. tostring(image.path))
        else
            -- 必须深拷贝：applyDevelopSettings 可能复用同一个表，
            -- 直接保存返回值会把“已修改”的设置当成原始值恢复。
            local original = copy_table(photo:getDevelopSettings())
            log('photo: ' .. image.path)
            for _, control in ipairs(job.controls or {}) do
                local destination = LrPathUtils.child(
                    job.output_root,
                    tostring(image.key) .. '_' .. tostring(control.name)
                )
                if not job.dry_run then
                    catalog:withWriteAccessDo('FDXMP reference', function()
                        photo:applyDevelopSettings(control.settings or {}, 'FDXMP reference')
                    end)
                    export_photo(photo, destination)
                    log('exported: ' .. destination)
                else
                    log('dry run: ' .. destination)
                end
            end
            if job.restore_settings ~= false then
                catalog:withWriteAccessDo('FDXMP restore', function()
                    photo:applyDevelopSettings(original, 'FDXMP restore')
                end)
                log('restored')
            else
                log('kept applied settings')
            end
        end
    end
end

log('plugin init')

if file_exists(JOB) then
    LrTasks.startAsyncTask(function()
        log('job found')
        local job, err = load_job()
        if not job then
            log('job load failed: ' .. tostring(err))
            return
        end
        run_job(job)
        log('job done')
        if job.auto_shutdown then
            LrTasks.execute(function()
                LrApplication.shutdown()
            end)
        end
    end)
end
