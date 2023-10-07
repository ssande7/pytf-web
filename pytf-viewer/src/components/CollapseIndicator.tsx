import React from 'react';
import ExpandMoreIcon from '@mui/icons-material/ExpandMore';
import ExpandLessIcon from '@mui/icons-material/ExpandLess';

interface ICollapse {
  visible: boolean
}

const CollapseIndicator: React.FC<ICollapse> = ({ visible }) => {
  return (
    <div className="icon-button header">
      {visible ? <ExpandLessIcon/> : <ExpandMoreIcon/> }
    </div>
  );
}

export default CollapseIndicator;
