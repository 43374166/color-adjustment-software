local LrTasks = import 'LrTasks'
local LrApplication = import 'LrApplication'

local LOG = [[D:\color-adjustment-software\.tmp\lr-reference-export.log]]

LrTasks.startAsyncTask(function()
    local file = io.open(LOG, 'a')
    if file then
        file:write(os.date('%Y-%m-%d %H:%M:%S') .. ' menu clicked\n')
        file:close()
    end
    local catalog = LrApplication.activeCatalog()
    if file then
        file = io.open(LOG, 'a')
        file:write('photos=' .. tostring(#catalog:getAllPhotos()) .. '\n')
        file:close()
    end
end)
